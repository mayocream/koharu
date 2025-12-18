use anyhow::{Context, Result};
use image::{DynamicImage, RgbaImage, imageops};
use koharu_renderer::{
    font::{FamilyName, Font, FontBook, Properties},
    layout::{LayoutRun, TextLayout, WritingMode},
    renderer::{RenderOptions, SkiaRenderer},
};

use crate::{
    image::SerializableDynamicImage,
    state::{Document, TextBlock, TextStyle},
};

#[derive(Clone, Copy)]
struct PaintInfo {
    fill: [u8; 4],
}

struct BlockContext {
    style: TextStyle,
    font: Font,
    writing_mode: WritingMode,
}

struct BlockPlan {
    index: usize,
    context: BlockContext,
    best_fit_size: f32,
}

pub struct Renderer {
    renderer: SkiaRenderer,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            renderer: SkiaRenderer::new(),
        }
    }

    pub fn render(&self, doc: &mut Document) -> Result<()> {
        let Some(inpainted) = doc.inpainted.as_deref() else {
            tracing::warn!("No inpainted image found for rendering");
            return Ok(());
        };

        let mut rendered = inpainted.to_rgba8();
        let renderable_indices: Vec<usize> = doc
            .text_blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| block.translation.as_ref().is_some_and(|x| !x.is_empty()))
            .map(|(index, _)| index)
            .collect();

        let mut plans = renderable_indices
            .into_iter()
            .map(|index| {
                let block = &doc.text_blocks[index];
                let style = block.style.clone().unwrap_or_default();
                let text = block.translation.as_deref().unwrap_or_default();
                let writing_mode = Self::writing_mode_for_block(block, text);
                let font = self.select_font(&style).context("failed to select font")?;

                Ok(BlockPlan {
                    index,
                    context: BlockContext {
                        style,
                        font,
                        writing_mode,
                    },
                    best_fit_size: 0.0,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let mut average_samples = Vec::new();
        for plan in plans.iter_mut() {
            let block = &doc.text_blocks[plan.index];
            let text = block.translation.as_deref().unwrap_or_default();
            let best_fit = if let Some(font_size) = plan.context.style.font_size {
                font_size
            } else {
                self.best_fit_font_size(block, &plan.context, text)?
            };

            plan.best_fit_size = best_fit;
            average_samples.push(best_fit);
        }

        let default_font_size = Self::filtered_average(&average_samples);

        for plan in &plans {
            let block = &doc.text_blocks[plan.index];
            let text = block.translation.as_deref().unwrap_or_default();
            self.render_block(block, text, plan, default_font_size, &mut rendered)?;
        }

        doc.rendered = Some(SerializableDynamicImage::from(DynamicImage::ImageRgba8(
            rendered,
        )));

        Ok(())
    }

    fn select_font(&self, style: &TextStyle) -> Result<Font> {
        let families: Vec<FamilyName> = style
            .font_families
            .iter()
            .map(|name| FamilyName::Title(name.clone()))
            .collect();

        // `FontBook` (font-kit MultiSource) is not `Send + Sync`, so we keep it local per call.
        // This keeps `Renderer` compatible with Tauri state (`Send + Sync`).
        let book = FontBook::new();
        book.query(&families, &Properties::default())
    }

    fn render_block(
        &self,
        block: &TextBlock,
        text: &str,
        plan: &BlockPlan,
        shared_default: Option<f32>,
        image: &mut RgbaImage,
    ) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        let (layout, layout_width, layout_height, font_size) =
            self.layout_for_render(block, plan, shared_default, text)?;

        if layout.lines.is_empty() {
            return Ok(());
        }

        let glyph_image = self.renderer.render(
            &layout,
            plan.context.writing_mode,
            &plan.context.font,
            &RenderOptions {
                color: Self::paint_for_block(block, &plan.context.style).fill,
                font_size,
                padding: 0.0,
                ..Default::default()
            },
        )?;

        let x_offset = ((block.width - layout_width).max(0.0) / 2.0).floor();
        let y_offset = ((block.height - layout_height).max(0.0) / 2.0).floor();
        let dest_x = (block.x + x_offset).floor().max(0.0) as i64;
        let dest_y = (block.y + y_offset).floor().max(0.0) as i64;

        imageops::overlay(image, &glyph_image, dest_x, dest_y);
        Ok(())
    }

    fn layout_for_render(
        &self,
        block: &TextBlock,
        plan: &BlockPlan,
        shared_default: Option<f32>,
        text: &str,
    ) -> Result<(LayoutRun, f32, f32, f32)> {
        let use_default = plan.context.style.font_size.is_none();
        let mut font_size = plan
            .context
            .style
            .font_size
            .or(shared_default)
            .unwrap_or(plan.best_fit_size);

        let mut layout = self.layout_with_size(block, &plan.context, font_size, text)?;
        let (mut width, mut height) = (layout.width, layout.height);
        let mut fits = Self::fits((width, height), block);

        if use_default && shared_default.is_some() && !fits {
            font_size = plan.best_fit_size;
            layout = self.layout_with_size(block, &plan.context, font_size, text)?;
            width = layout.width;
            height = layout.height;
            fits = Self::fits((width, height), block);
        }

        if !fits {
            tracing::debug!(
                "Layout exceeds block bounds; width {:.1} height {:.1} vs block {}x{}",
                width,
                height,
                block.width,
                block.height
            );
        }

        Ok((layout, width, height, font_size))
    }

    fn best_fit_font_size(
        &self,
        block: &TextBlock,
        context: &BlockContext,
        text: &str,
    ) -> Result<f32> {
        if text.trim().is_empty() {
            return Ok(context.style.font_size.unwrap_or(12.0));
        }

        let mut low = 8.0;
        let mut high = 200.0;
        let epsilon = 0.5;
        let min_size = if Self::contains_cjk(text) { 16.0 } else { 12.0 };

        while high - low > epsilon {
            let mid = (low + high) / 2.0;
            let layout = self.layout_with_size(block, context, mid, text)?;
            let (width, height) = (layout.width, layout.height);

            if layout.lines.is_empty() {
                high = mid;
                continue;
            }

            if Self::fits((width, height), block) {
                low = mid;
            } else {
                high = mid;
            }
        }

        Ok(low.max(min_size))
    }

    fn layout_with_size(
        &self,
        block: &TextBlock,
        context: &BlockContext,
        size: f32,
        text: &str,
    ) -> Result<LayoutRun> {
        let layout = if context.writing_mode == WritingMode::VerticalRl {
            TextLayout::new(&context.font, size)
                .with_writing_mode(WritingMode::VerticalRl)
                .with_max_height(block.height)
        } else {
            TextLayout::new(&context.font, size).with_max_width(block.width)
        };

        layout
            .run(text)
            .with_context(|| "failed to layout text for rendering")
    }

    fn fits(extents: (f32, f32), block: &TextBlock) -> bool {
        let (w, h) = extents;
        w <= block.width + f32::EPSILON && h <= block.height + f32::EPSILON
    }

    fn writing_mode_for_block(block: &TextBlock, text: &str) -> WritingMode {
        if !Self::contains_cjk(text) || block.width >= block.height {
            WritingMode::Horizontal
        } else {
            WritingMode::VerticalRl
        }
    }

    fn contains_cjk(text: &str) -> bool {
        text.chars().any(Self::is_cjk)
    }

    fn is_cjk(c: char) -> bool {
        matches!(
            c,
            '\u{4E00}'..='\u{9FFF}'
                | '\u{3400}'..='\u{4DBF}'
                | '\u{3040}'..='\u{309F}'
                | '\u{30A0}'..='\u{30FF}'
                | '\u{AC00}'..='\u{D7AF}'
        )
    }

    fn paint_for_block(block: &TextBlock, style: &TextStyle) -> PaintInfo {
        let mut fill = style.color;

        if let Some(info) = block.font_info.as_ref() {
            fill = [
                info.text_color[0],
                info.text_color[1],
                info.text_color[2],
                255,
            ];
        }

        PaintInfo { fill }
    }

    fn filtered_average(samples: &[f32]) -> Option<f32> {
        let mut values: Vec<f32> = samples.iter().copied().filter(|v| v.is_finite()).collect();
        if values.is_empty() {
            return None;
        }

        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let q1 = Self::quantile(&values, 0.25);
        let q3 = Self::quantile(&values, 0.75);
        let iqr = q3 - q1;
        let lower = q1 - 1.5 * iqr;
        let upper = q3 + 1.5 * iqr;

        let filtered: Vec<f32> = values
            .iter()
            .copied()
            .filter(|v| *v >= lower && *v <= upper)
            .collect();

        let final_values = if filtered.is_empty() {
            values
        } else {
            filtered
        };
        let sum: f32 = final_values.iter().copied().sum();
        Some(sum / final_values.len() as f32)
    }

    fn quantile(values: &[f32], percentile: f32) -> f32 {
        if values.is_empty() {
            return 0.0;
        }

        let clamped = percentile.clamp(0.0, 1.0);
        let pos = clamped * (values.len() as f32 - 1.0);
        let lower = pos.floor() as usize;
        let upper = pos.ceil() as usize;

        if lower == upper {
            values[lower]
        } else {
            let weight = pos - lower as f32;
            values[lower] * (1.0 - weight) + values[upper] * weight
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}
