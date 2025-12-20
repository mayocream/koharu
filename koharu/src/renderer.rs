use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use image::{DynamicImage, RgbaImage, imageops};
use koharu_renderer::{
    font::{FamilyName, Font, FontBook, Properties},
    layout::{LayoutRun, TextLayout, WritingMode},
    renderer::{RenderOptions, WgpuRenderer},
};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

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
}

pub struct Renderer {
    fontbook: Arc<Mutex<FontBook>>,
    renderer: WgpuRenderer,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            fontbook: Arc::new(Mutex::new(FontBook::new())),
            renderer: WgpuRenderer::new()?,
        })
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

        let plans = renderable_indices
            .iter()
            .map(|index| {
                let block = &doc.text_blocks[*index];
                let style = block.style.clone().unwrap_or_default();
                let text = block.translation.as_deref().unwrap_or_default();
                let writing_mode = Self::writing_mode_for_block(block, text);
                let font = self.select_font(&style)?;

                Ok(BlockPlan {
                    index: *index,
                    context: BlockContext {
                        style,
                        font,
                        writing_mode,
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let render_results = plans
            .par_iter()
            .map(|plan| {
                let block = &doc.text_blocks[plan.index];
                let text = block.translation.as_deref().unwrap_or_default();
                self.render_block(block, text, plan)
                    .with_context(|| format!("failed to render block at index {}", plan.index))
            })
            .collect::<Result<Vec<_>>>()?;

        for (glyph_image, dest_x, dest_y) in render_results {
            imageops::overlay(&mut rendered, &glyph_image, dest_x, dest_y);
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

        self.fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("failed to lock fontbook"))?
            .query(&families, &Properties::default())
    }

    fn render_block(
        &self,
        block: &TextBlock,
        text: &str,
        plan: &BlockPlan,
    ) -> Result<(RgbaImage, i64, i64)> {
        let (layout, layout_width, layout_height, font_size) =
            self.layout_for_render(block, plan, text)?;

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

        Ok((glyph_image, dest_x, dest_y))
    }

    fn layout_for_render(
        &self,
        block: &TextBlock,
        plan: &BlockPlan,
        text: &str,
    ) -> Result<(LayoutRun, f32, f32, f32)> {
        let font_size = plan.context.style.font_size;

        let layout = self.layout_with_size(block, &plan.context, font_size, text)?;
        let (width, height) = (layout.width, layout.height);
        let fits = Self::fits((width, height), block);

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

        if let Some(info) = block.font_prediction.as_ref() {
            fill = [
                info.text_color[0],
                info.text_color[1],
                info.text_color[2],
                255,
            ];
        }

        PaintInfo { fill }
    }
}
