use std::sync::Arc;

use anyhow::Result;
use image::{DynamicImage, RgbaImage};
use koharu_renderer::{
    Script,
    font::{Font, FontBook, Language},
    layout::{LayoutRequest, Layouter, Orientation, calculate_bounds},
    render::{RenderRequest, Renderer},
};
use tokio::sync::Mutex;

use crate::{
    image::SerializableDynamicImage,
    state::{Document, TextBlock, TextStyle},
};

#[derive(Clone, Copy)]
struct PaintInfo {
    fill: koharu_renderer::types::Color,
    stroke: Option<koharu_renderer::types::Color>,
    stroke_ratio: Option<f32>,
    stroke_width_px: f32,
}

struct BlockContext {
    style: TextStyle,
    fonts: Vec<Font>,
    script: Script,
    direction: Orientation,
    paint: PaintInfo,
}

struct BlockPlan {
    index: usize,
    context: BlockContext,
    best_fit_size: f32,
}

pub struct TextRenderer {
    fontbook: Arc<Mutex<FontBook>>,
    renderer: Arc<Mutex<Renderer>>,
    layouter: Arc<Mutex<Layouter>>,
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextRenderer {
    pub fn new() -> Self {
        Self {
            fontbook: Arc::new(Mutex::new(FontBook::new())),
            renderer: Arc::new(Mutex::new(Renderer::new())),
            layouter: Arc::new(Mutex::new(Layouter::new())),
        }
    }

    pub async fn available_fonts(&self) -> Vec<String> {
        let fontbook = self.fontbook.lock().await;
        fontbook
            .filter_by_language(&[
                Language::Chinese_PeoplesRepublicOfChina,
                Language::Chinese_Taiwan,
                Language::Chinese_HongKongSAR,
                Language::English_UnitedStates,
            ])
            .iter()
            .flat_map(|face| face.families.iter().map(|f| f.0.clone()))
            .collect()
    }

    pub async fn render(&self, doc: &mut Document) -> Result<()> {
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

        let mut plans = {
            let mut fontbook = self.fontbook.lock().await;
            renderable_indices
                .into_iter()
                .map(|index| {
                    let block = &doc.text_blocks[index];
                    let style = block.style.clone().unwrap_or_default();
                    let paint = Self::paint_for_block(block, &style);
                    let (collected_faces, script) = fontbook.filter_by_families_for_text(
                        &style.font_families,
                        block.translation.as_deref().unwrap_or_default(),
                    );

                    let fonts = collected_faces
                        .iter()
                        .filter_map(|face| fontbook.font(face).ok())
                        .collect::<Vec<_>>();

                    let direction = Self::orientation_for_block(script, block);

                    BlockPlan {
                        index,
                        context: BlockContext {
                            style,
                            fonts,
                            script,
                            direction,
                            paint,
                        },
                        best_fit_size: 0.0,
                    }
                })
                .collect::<Vec<_>>()
        };

        let mut average_samples = Vec::new();
        {
            let mut layouter = self.layouter.lock().await;
            for plan in plans.iter_mut() {
                let block = &doc.text_blocks[plan.index];
                let best_fit = if let Some(font_size) = plan.context.style.font_size {
                    font_size
                } else {
                    Self::best_fit_font_size(&mut layouter, block, &plan.context)?
                };

                plan.best_fit_size = best_fit;
                average_samples.push(best_fit);
            }
        }

        let default_font_size = Self::filtered_average(&average_samples);

        for plan in &plans {
            let block = &doc.text_blocks[plan.index];
            self.render_block(block, plan, default_font_size, &mut rendered)
                .await?;
        }

        doc.rendered = Some(SerializableDynamicImage::from(DynamicImage::ImageRgba8(
            rendered,
        )));

        Ok(())
    }

    async fn render_block(
        &self,
        block: &TextBlock,
        plan: &BlockPlan,
        shared_default: Option<f32>,
        image: &mut RgbaImage,
    ) -> Result<()> {
        let (glyphs, x_offset, y_offset, font_size) = {
            let mut layouter = self.layouter.lock().await;
            Self::layout_for_render(&mut layouter, block, plan, shared_default)?
        };

        let mut renderer = self.renderer.lock().await;
        renderer.render(&mut RenderRequest {
            layout: &glyphs,
            image,
            x: if plan.context.direction == Orientation::Horizontal {
                block.x + x_offset
            } else {
                block.x + block.width - font_size - x_offset
            },
            y: y_offset + block.y + font_size,
            font_size,
            color: plan.context.paint.fill,
            stroke_color: plan.context.paint.stroke,
            stroke_width: Self::stroke_width_for_block(plan, font_size),
            direction: plan.context.direction,
        })?;

        Ok(())
    }

    fn layout_for_render(
        layouter: &mut Layouter,
        block: &TextBlock,
        plan: &BlockPlan,
        shared_default: Option<f32>,
    ) -> Result<(koharu_renderer::layout::LayoutResult, f32, f32, f32)> {
        let use_default = plan.context.style.font_size.is_none();
        let mut font_size = plan
            .context
            .style
            .font_size
            .or(shared_default)
            .unwrap_or(plan.best_fit_size);

        let mut glyphs = Self::layout_with_size(layouter, block, &plan.context, font_size)?;
        let (mut x_offset, mut y_offset, mut fits) =
            Self::layout_offsets(&glyphs, block, plan.context.direction);

        if use_default && shared_default.is_some() && !fits {
            font_size = plan.best_fit_size;
            glyphs = Self::layout_with_size(layouter, block, &plan.context, font_size)?;
            let offsets = Self::layout_offsets(&glyphs, block, plan.context.direction);
            x_offset = offsets.0;
            y_offset = offsets.1;
            fits = offsets.2;
        }

        tracing::debug!("Determined font size: {}", font_size);
        if !fits {
            tracing::debug!(
                "Layout exceeds block bounds; using offsets {:?}",
                (x_offset, y_offset)
            );
        }

        Ok((glyphs, x_offset, y_offset, font_size))
    }

    fn best_fit_font_size(
        layouter: &mut Layouter,
        block: &TextBlock,
        context: &BlockContext,
    ) -> Result<f32> {
        let mut low = 8.0;
        let mut high = 200.0;
        let epsilon = 0.5;
        let smallest_readable_size = if context.script == Script::Latin {
            12.0
        } else {
            16.0
        };

        while high - low > epsilon {
            let mid = (low + high) / 2.0;
            if mid < smallest_readable_size {
                low = smallest_readable_size;
                break;
            }

            let glyphs = Self::layout_with_size(layouter, block, context, mid)?;
            if glyphs.is_empty() {
                high = mid;
                continue;
            }

            let fits = Self::layout_offsets(&glyphs, block, context.direction).2;
            if fits {
                low = mid;
            } else {
                high = mid;
            }
        }

        Ok((low * 0.95).max(smallest_readable_size))
    }

    fn layout_with_size(
        layouter: &mut Layouter,
        block: &TextBlock,
        context: &BlockContext,
        size: f32,
    ) -> Result<koharu_renderer::layout::LayoutResult> {
        layouter.layout(&LayoutRequest {
            text: block.translation.as_deref().unwrap_or_default(),
            fonts: &context.fonts,
            font_size: size,
            line_height: context.style.line_height * size,
            script: context.script,
            max_primary_axis: if context.direction == Orientation::Horizontal {
                block.width
            } else {
                block.height
            },
            direction: context.direction,
        })
    }

    fn layout_offsets(
        layout: &koharu_renderer::layout::LayoutResult,
        block: &TextBlock,
        direction: Orientation,
    ) -> (f32, f32, bool) {
        if layout.is_empty() {
            return (0.0, 0.0, false);
        }

        let (min_x, min_y, max_x, max_y) = calculate_bounds(layout);
        let fits = max_y - min_y <= block.height && max_x - min_x <= block.width;

        let x_offset = ((block.width - (max_x - min_x)) / 2.).max(0.);
        let y_offset = ((block.height - (max_y - min_y)) / 2.).max(0.);

        match direction {
            Orientation::Horizontal => (0.0, y_offset.floor(), fits),
            Orientation::Vertical => (x_offset.floor(), 0.0, fits),
        }
    }

    fn orientation_for_block(script: Script, block: &TextBlock) -> Orientation {
        if block.width > block.height || script == Script::Latin {
            Orientation::Horizontal
        } else {
            Orientation::Vertical
        }
    }

    fn stroke_width_for_block(plan: &BlockPlan, font_size: f32) -> f32 {
        let mut width = plan.context.paint.stroke_width_px.max(0.0);
        if let Some(ratio) = plan.context.paint.stroke_ratio {
            width = (ratio * font_size).clamp(0.0, font_size * 0.6);
        }
        width
    }

    fn paint_for_block(block: &TextBlock, style: &TextStyle) -> PaintInfo {
        let mut fill = style.color;
        let mut stroke = None;
        let mut stroke_ratio = None;
        let mut stroke_width_px = 0.0;

        if let Some(info) = block.font_info.as_ref() {
            fill = [
                info.text_color[0],
                info.text_color[1],
                info.text_color[2],
                255,
            ];

            if info.stroke_width_px > 0.0 {
                stroke = Some([
                    info.stroke_color[0],
                    info.stroke_color[1],
                    info.stroke_color[2],
                    255,
                ]);
                stroke_width_px = info.stroke_width_px.max(0.0);
                if info.font_size_px > 0.0 {
                    stroke_ratio = Some((info.stroke_width_px / info.font_size_px).max(0.0));
                }
            }
        }

        PaintInfo {
            fill,
            stroke,
            stroke_ratio,
            stroke_width_px,
        }
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
