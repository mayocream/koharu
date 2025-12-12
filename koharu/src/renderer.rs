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

        // Calculate average font size across all blocks (excluding outliers)
        let average_font_size = self.calculate_average_font_size(doc).await?;
        tracing::debug!("Average font size for document: {}", average_font_size);

        for block in &doc.text_blocks {
            if block.translation.as_ref().is_none_or(|x| x.is_empty()) {
                continue;
            }

            self.render_block(block, &mut rendered, Some(average_font_size)).await?;
        }

        doc.rendered = Some(SerializableDynamicImage::from(DynamicImage::ImageRgba8(
            rendered,
        )));

        Ok(())
    }

    /// Calculate the average font size across all blocks, excluding outliers
    async fn calculate_average_font_size(&self, doc: &Document) -> Result<f32> {
        let mut font_sizes = Vec::new();

        for block in &doc.text_blocks {
            if block.translation.as_ref().is_none_or(|x| x.is_empty()) {
                continue;
            }

            let font_size = self.calculate_block_font_size(block).await?;
            font_sizes.push(font_size);
        }

        if font_sizes.is_empty() {
            return Ok(16.0); // Default font size
        }

        // Sort to calculate quartiles
        font_sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Remove outliers using IQR method
        let q1_idx = font_sizes.len() / 4;
        let q3_idx = (font_sizes.len() * 3) / 4;
        
        if font_sizes.len() >= 4 {
            let q1 = font_sizes[q1_idx];
            let q3 = font_sizes[q3_idx];
            let iqr = q3 - q1;
            let lower_bound = q1 - 1.5 * iqr;
            let upper_bound = q3 + 1.5 * iqr;

            // Filter out outliers
            font_sizes.retain(|&size| size >= lower_bound && size <= upper_bound);
        }

        if font_sizes.is_empty() {
            return Ok(16.0); // Default font size if all were outliers
        }

        // Calculate average
        let sum: f32 = font_sizes.iter().sum();
        Ok(sum / font_sizes.len() as f32)
    }

    /// Calculate the optimal font size for a single block
    async fn calculate_block_font_size(&self, block: &TextBlock) -> Result<f32> {
        let mut fontbook = self.fontbook.lock().await;
        let style = block.style.clone().unwrap_or_default();

        // If font size is explicitly set, use it
        if let Some(font_size) = style.font_size {
            return Ok(font_size);
        }

        let (collected_faces, script) = fontbook.filter_by_families_for_text(
            &style.font_families,
            block.translation.as_ref().unwrap_or(&"".to_string()),
        );

        let fonts = collected_faces
            .iter()
            .filter_map(|face| fontbook.font(face).ok())
            .collect::<Vec<_>>();

        let direction = if block.width > block.height || script == Script::Latin {
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

        // Binary search for optimal font size
        let mut low = 8.0;
        let mut high = 200.0;
        let epsilon = 0.5;
        let smallest_readable_size = if script == Script::Latin { 12 } else { 16 };

        while high - low > epsilon {
            let mid = (low + high) / 2.0;
            if mid < smallest_readable_size as f32 {
                low = smallest_readable_size as f32;
                break;
            }
            let glyphs = layout_with_size(mid)?;

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
        }

        Ok(low * 0.95)
    }

    async fn render_block(&self, block: &TextBlock, image: &mut RgbaImage, override_font_size: Option<f32>) -> Result<()> {
        let mut fontbook = self.fontbook.lock().await;
        let style = block.style.clone().unwrap_or_default();

        let (collected_faces, script) = fontbook.filter_by_families_for_text(
            &style.font_families,
            block.translation.as_ref().unwrap_or(&"".to_string()),
        );

        let fonts = collected_faces
            .iter()
            .filter_map(|face| fontbook.font(face).ok())
            .collect::<Vec<_>>();

        let direction = if block.width > block.height || script == Script::Latin {
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

        let (font_size, x_offset, y_offset) = if let Some(override_size) = override_font_size {
            // Use the average font size provided, calculate offsets to center the text
            let glyphs = layout_with_size(override_size)?;
            let mut x_offset = 0.;
            let mut y_offset = 0.;
            
            if !glyphs.is_empty() {
                let (min_x, min_y, max_x, max_y) = calculate_bounds(&glyphs);
                x_offset = ((block.width - (max_x - min_x)) / 2.).max(0.);
                y_offset = ((block.height - (max_y - min_y)) / 2.).max(0.);
            }
            
            if direction == Orientation::Horizontal {
                (override_size, 0., y_offset.floor())
            } else {
                (override_size, x_offset.floor(), 0.)
            }
        } else if let Some(font_size) = style.font_size {
            (font_size, 0., 0.)
        } else {
            // binary search for optimal font size to fit the text block
            let mut low = 8.0;
            let mut high = 200.0;
            let epsilon = 0.5; // Convergence threshold
            let smallest_readable_size = if script == Script::Latin { 12 } else { 16 }; // px

            // the compesation offsets to center the rendered text inside the block
            let mut x_offset = 0.;
            let mut y_offset = 0.;

            // Binary search with proper convergence
            while high - low > epsilon {
                let mid = (low + high) / 2.0;
                // TODO: fix horizonal latin text measuring issue
                if mid < smallest_readable_size as f32 {
                    low = smallest_readable_size as f32;
                    break;
                }
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
                x_offset = ((block.width - (max_x - min_x)) / 2.).max(0.);
                y_offset = ((block.height - (max_y - min_y)) / 2.).max(0.);
            }

            tracing::debug!(
                "font size {}, {} x {}",
                low * 0.95,
                block.width,
                block.height
            );

            // Use a slightly smaller size to ensure it fits with some margin
            if direction == Orientation::Horizontal {
                (low * 0.95, 0., y_offset.floor())
            } else {
                (low * 0.95, x_offset.floor(), 0.)
            }
        };

        tracing::debug!("Determined font size: {}", font_size);

        let glyphs = layout_with_size(font_size)?;
        let mut renderer = self.renderer.lock().await;
        renderer.render(&mut RenderRequest {
            layout: &glyphs,
            image,
            x: if direction == Orientation::Horizontal {
                block.x + x_offset
            } else {
                block.x + block.width - font_size - x_offset
            },
            y: y_offset + block.y + font_size,
            font_size,
            color: style.color,
            direction,
        })?;

        Ok(())
    }
}
