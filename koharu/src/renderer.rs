use std::sync::Arc;

use anyhow::Result;
use image::{DynamicImage, RgbaImage};
use koharu_renderer::{
    Script,
    font::{FontBook, Language},
    layout::{LayoutRequest, Layouter, Orientation, calculate_bounds},
    render::{RenderRequest, Renderer},
};
use tokio::sync::Mutex;

use crate::{
    image::SerializableDynamicImage,
    state::{Document, TextBlock},
};

pub struct TextRenderer {
    fontbook: Arc<Mutex<FontBook>>,
    renderer: Arc<Mutex<Renderer>>,
    layouter: Arc<Mutex<Layouter>>,
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
            .map(|face| face.families.iter().map(|f| f.0.clone()))
            .flatten()
            .collect()
    }

    pub async fn render(&self, doc: &mut Document) -> Result<()> {
        let Some(inpainted) = doc.inpainted.as_deref() else {
            tracing::warn!("No inpainted image found for rendering");
            return Ok(());
        };

        let mut rendered = inpainted.to_rgba8();

        for block in &doc.text_blocks {
            if block.translation.as_ref().is_none_or(|x| x.is_empty()) {
                continue;
            }

            self.render_block(block, &mut rendered).await?;
        }

        doc.rendered = Some(SerializableDynamicImage::from(DynamicImage::ImageRgba8(
            rendered,
        )));

        Ok(())
    }

    async fn render_block(&self, block: &TextBlock, image: &mut RgbaImage) -> Result<()> {
        let mut fontbook = self.fontbook.lock().await;
        let style = block.style.clone();


        let (collected_faces, script) = fontbook
            .filter_by_families_for_text(&style.font_families, block.translation.as_ref().unwrap_or(&"".to_string()));

        let fonts = collected_faces
            .iter()
            .filter_map(|face| fontbook.font(face).ok())
            .collect::<Vec<_>>();

        let direction = if block.width > block.height {
            Orientation::Horizontal
        } else {
            Orientation::Vertical
        };

        let mut layouter = self.layouter.lock().await;
        let mut layout_with_size = |size: f32| -> Result<_> {
            layouter.layout(&LayoutRequest {
                text: block.translation.as_deref().unwrap_or_default(),
                fonts: &fonts,
                font_size: size,
                line_height: style.line_height * size,
                script,
                max_primary_axis: if direction == Orientation::Horizontal {
                    block.width
                } else {
                    block.height
                },
                direction,
            })
        };

        let font_size = if let Some(font_size) = style.font_size {
            font_size
        } else {
            // binary search for optimal font size to fit the text block
            let mut low = 8.0;
            let mut high = 200.0;
            let epsilon = 0.5; // Convergence threshold
            let smallest_readable_size = if script == Script::Latin {
                12
            } else {
                16
            }; // px

            // Binary search with proper convergence
            while high - low > epsilon && high > smallest_readable_size as f32 {
                let mid = (low + high) / 2.0;
                let glyphs = layout_with_size(mid)?;

                // Handle empty layout
                if glyphs.is_empty() {
                    high = mid;
                    continue;
                }

                let (min_x, min_y, max_x, max_y) = calculate_bounds(&glyphs);
                if max_y - min_y <= block.height && max_x - min_x <= block.width {
                    low = mid;
                } else {
                    high = mid;
                }
                tracing::info!("font size search: low={}, high={}, mid={}, height={}, width={}", low, high, mid, max_y-min_y, max_x-min_x);
            }

            // Use a slightly smaller size to ensure it fits with some margin
            low * 0.95
        };

        tracing::info!("Determined font size: {}", font_size);

        let glyphs = layout_with_size(font_size)?;
        let mut renderer = self.renderer.lock().await;
        renderer.render(&mut RenderRequest {
            layout: &glyphs,
            image,
            x: if direction == Orientation::Horizontal {
                block.x
            } else {
                block.x + block.width - font_size
            },
            y: block.y + font_size,
            font_size,
            color: style.color,
        })?;

        Ok(())
    }
}
