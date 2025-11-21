use std::sync::Arc;

use anyhow::Result;
use image::{DynamicImage, RgbaImage};
use koharu_core::image::SerializableDynamicImage;
use koharu_renderer::{
    Script,
    font::{FontBook, Language},
    layout::{LayoutRequest, Layouter, Orientation, calculate_bounds},
    render::{RenderRequest, Renderer},
};
use tokio::sync::Mutex;

use crate::state::{Document, TextBlock};

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
            .filter_by_language(&Language::Chinese_PeoplesRepublicOfChina)
            .iter()
            .map(|face| face.families.iter().map(|f| f.0.clone()))
            .flatten()
            .collect()
    }

    pub async fn render(&self, doc: &mut Document) -> Result<()> {
        let Some(inpainted) = doc.inpainted.as_deref() else {
            return Ok(());
        };

        let mut rendered = inpainted.to_rgba8();

        for block in &doc.text_blocks {
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
        let fonts = fontbook
            .filter_by_families(&style.font_families)
            .iter()
            .filter_map(|face| fontbook.font(face).ok())
            .collect::<Vec<_>>();

        let direction = if block.width >= block.height {
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
                script: Script::Han,
                max_primary_axis: if direction == Orientation::Horizontal {
                    block.width
                } else {
                    block.height
                },
                direction: direction,
            })
        };

        let font_size = if let Some(font_size) = style.font_size {
            font_size
        } else {
            // binary search for optimal font size to fit the text block
            let mut low = 8.0;
            let mut high = 200.0;
            let epsilon = 0.5; // Convergence threshold

            // Binary search with proper convergence
            while high - low > epsilon {
                let mid = (low + high) / 2.0;
                let glyphs = layout_with_size(mid)?;

                // Handle empty layout
                if glyphs.is_empty() {
                    high = mid;
                    continue;
                }

                let (min_x, min_y, max_x, max_y) = calculate_bounds(&glyphs);
                let width = max_x - min_x;
                let height = max_y - min_y;

                // Check if text fits within the block
                let fits = match direction {
                    Orientation::Horizontal => width <= block.width && height <= block.height,
                    Orientation::Vertical => height <= block.height && width <= block.width,
                };

                if fits {
                    low = mid;
                } else {
                    high = mid;
                }
            }

            // Use a slightly smaller size to ensure it fits with some margin
            low * 0.95
        };

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
            font_size: font_size,
            color: style.color,
        })?;

        Ok(())
    }
}
