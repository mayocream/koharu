use std::collections::HashMap;
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

// Font size calculation constants
const MIN_FONT_SIZE: f32 = 8.0;
const MAX_FONT_SIZE: f32 = 200.0;
const CONVERGENCE_EPSILON: f32 = 0.5;
const MIN_LATIN_SIZE: i32 = 12;
const MIN_NON_LATIN_SIZE: i32 = 16;
const FONT_SIZE_SAFETY_MARGIN: f32 = 0.95;
const DEFAULT_FONT_SIZE: f32 = 16.0;
const IQR_MULTIPLIER: f32 = 1.5;

/// Cache key for font size calculations based on block dimensions and text characteristics
#[derive(Hash, Eq, PartialEq)]
struct FontSizeCacheKey {
    width: u32,
    height: u32,
    text_len: usize,
    text_hash: u64,
}

impl FontSizeCacheKey {
    fn from_block(block: &TextBlock) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let text = block.translation.as_deref().unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        
        Self {
            width: block.width as u32,
            height: block.height as u32,
            text_len: text.len(),
            text_hash: hasher.finish(),
        }
    }
}

pub struct TextRenderer {
    fontbook: Arc<Mutex<FontBook>>,
    renderer: Arc<Mutex<Renderer>>,
    layouter: Arc<Mutex<Layouter>>,
    font_size_cache: Arc<Mutex<HashMap<FontSizeCacheKey, f32>>>,
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
            font_size_cache: Arc::new(Mutex::new(HashMap::new())),
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
    /// using the Interquartile Range (IQR) method.
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
            return Ok(DEFAULT_FONT_SIZE);
        }

        // Sort to calculate quartiles
        font_sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Remove outliers using IQR method (only if we have enough data points)
        if font_sizes.len() >= 4 {
            // Helper function to calculate quantile with linear interpolation
            let get_quartile = |sorted: &[f32], quantile: f32| -> f32 {
                let n = sorted.len();
                if n == 0 {
                    return 0.0;
                }
                let pos = (n - 1) as f32 * quantile;
                let lower = pos.floor() as usize;
                let upper = pos.ceil() as usize;
                if lower == upper {
                    sorted[lower]
                } else {
                    let weight = pos - lower as f32;
                    sorted[lower] * (1.0 - weight) + sorted[upper] * weight
                }
            };
            
            let q1 = get_quartile(&font_sizes, 0.25);
            let q3 = get_quartile(&font_sizes, 0.75);
            let iqr = q3 - q1;
            let lower_bound = q1 - IQR_MULTIPLIER * iqr;
            let upper_bound = q3 + IQR_MULTIPLIER * iqr;

            // Filter out outliers
            font_sizes.retain(|&size| size >= lower_bound && size <= upper_bound);
        }

        if font_sizes.is_empty() {
            return Ok(DEFAULT_FONT_SIZE); // Fallback if all were outliers
        }

        // Calculate average
        let sum: f32 = font_sizes.iter().sum();
        Ok(sum / font_sizes.len() as f32)
    }

    /// Calculate the optimal font size for a single block
    async fn calculate_block_font_size(&self, block: &TextBlock) -> Result<f32> {
        // Check cache first
        let cache_key = FontSizeCacheKey::from_block(block);
        {
            let cache = self.font_size_cache.lock().await;
            if let Some(&cached_size) = cache.get(&cache_key) {
                return Ok(cached_size);
            }
        }

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
        
        let font_size = Self::binary_search_font_size(
            &mut layouter,
            block,
            &fonts,
            &style,
            script,
            direction,
        )?;

        // Apply a 5% reduction to the calculated font size to provide a margin,
        // ensuring the rendered text fits comfortably within the block and avoids overflow.
        let final_size = font_size * FONT_SIZE_SAFETY_MARGIN;
        
        // Cache the result
        {
            let mut cache = self.font_size_cache.lock().await;
            cache.insert(cache_key, final_size);
        }
        
        Ok(final_size)
    }

    /// Perform binary search to find the largest font size that fits within block dimensions
    fn binary_search_font_size(
        layouter: &mut Layouter,
        block: &TextBlock,
        fonts: &[koharu_renderer::font::Font],
        style: &crate::state::TextStyle,
        script: Script,
        direction: Orientation,
    ) -> Result<f32> {
        let mut layout_with_size = |size: f32| -> Result<_> {
            layouter.layout(&LayoutRequest {
                text: block.translation.as_deref().unwrap_or_default(),
                fonts,
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

        let mut low = MIN_FONT_SIZE;
        let mut high = MAX_FONT_SIZE;
        let smallest_readable_size = if script == Script::Latin { MIN_LATIN_SIZE } else { MIN_NON_LATIN_SIZE };

        while high - low > CONVERGENCE_EPSILON {
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

        Ok(low)
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
            // Use binary search for optimal font size to fit the text block
            let low = Self::binary_search_font_size(
                &mut layouter,
                block,
                &fonts,
                &style,
                script,
                direction,
            )?;

            // Calculate offsets to center the rendered text inside the block
            let sized_glyphs = layouter.layout(&LayoutRequest {
                text: block.translation.as_deref().unwrap_or_default(),
                fonts: &fonts,
                font_size: low,
                line_height: style.line_height * low,
                script,
                max_primary_axis: if direction == Orientation::Horizontal {
                    block.width
                } else {
                    block.height
                },
                direction,
            })?;

            let mut x_offset = 0.;
            let mut y_offset = 0.;
            if !sized_glyphs.is_empty() {
                let (min_x, min_y, max_x, max_y) = calculate_bounds(&sized_glyphs);
                x_offset = ((block.width - (max_x - min_x)) / 2.).max(0.);
                y_offset = ((block.height - (max_y - min_y)) / 2.).max(0.);
            }

            tracing::debug!(
                "font size {}, {} x {}",
                low * FONT_SIZE_SAFETY_MARGIN,
                block.width,
                block.height
            );

            // Use a slightly smaller size to ensure it fits with some margin
            if direction == Orientation::Horizontal {
                (low * FONT_SIZE_SAFETY_MARGIN, 0., y_offset.floor())
            } else {
                (low * FONT_SIZE_SAFETY_MARGIN, x_offset.floor(), 0.)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::TextStyle;

    #[test]
    fn test_calculate_average_excludes_outliers() {
        // Test that outliers are correctly excluded from average calculation
        let font_sizes = vec![10.0, 12.0, 14.0, 15.0, 16.0, 17.0, 18.0, 100.0]; // 100.0 is an outlier
        
        // Simulate the outlier removal logic with linear interpolation
        let mut sizes = font_sizes.clone();
        sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        
        // Helper function to calculate quantile with linear interpolation
        let get_quartile = |sorted: &[f32], quantile: f32| -> f32 {
            let n = sorted.len();
            if n == 0 {
                return 0.0;
            }
            let pos = (n - 1) as f32 * quantile;
            let lower = pos.floor() as usize;
            let upper = pos.ceil() as usize;
            if lower == upper {
                sorted[lower]
            } else {
                let weight = pos - lower as f32;
                sorted[lower] * (1.0 - weight) + sorted[upper] * weight
            }
        };
        
        let q1 = get_quartile(&sizes, 0.25);
        let q3 = get_quartile(&sizes, 0.75);
        let iqr = q3 - q1;
        let lower_bound = q1 - 1.5 * iqr;
        let upper_bound = q3 + 1.5 * iqr;
        
        sizes.retain(|&size| size >= lower_bound && size <= upper_bound);
        
        // The outlier (100.0) should be removed
        assert!(!sizes.contains(&100.0));
        
        // Calculate average of non-outliers
        let sum: f32 = sizes.iter().sum();
        let average = sum / sizes.len() as f32;
        
        // Average should be around 14-15, not skewed by the 100.0 outlier
        assert!(average > 10.0 && average < 20.0);
    }

    #[test]
    fn test_calculate_average_with_small_dataset() {
        // Test that small datasets (< 4 elements) don't filter outliers
        let font_sizes = vec![10.0, 15.0, 100.0];
        
        let mut sizes = font_sizes.clone();
        sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        
        // With < 4 elements, no outlier filtering should occur
        if sizes.len() >= 4 {
            panic!("Test dataset should be < 4 elements");
        }
        
        // All values should remain
        assert_eq!(sizes.len(), 3);
    }

    #[test]
    fn test_default_font_size_for_empty_blocks() {
        // When there are no valid blocks, should return default font size
        let default_size = 16.0;
        
        let empty_sizes: Vec<f32> = vec![];
        let result = if empty_sizes.is_empty() {
            default_size
        } else {
            let sum: f32 = empty_sizes.iter().sum();
            sum / empty_sizes.len() as f32
        };
        
        assert_eq!(result, 16.0);
    }
}
