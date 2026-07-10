//! Bubble-mask fill fast path for inpainting.
//!
//! When the page already has a speech-bubble mask, we can skip the model for
//! simple bubbles: estimate the bubble background colour from the segmented
//! bubble region, fill the masked pixels that sit inside that bubble, and only
//! pass any remaining masked pixels to the actual inpainting model.

use std::collections::BTreeSet;

use image::{GrayImage, Luma, Rgb, RgbImage};

const SIMPLE_BG_THRESHOLD_LOW_VARIANCE: f64 = 10.0;
const SIMPLE_BG_THRESHOLD_HIGH_VARIANCE: f64 = 7.0;
const SIMPLE_BG_CHANNEL_STD_SWITCH: f64 = 1.0;

#[derive(Debug, Clone)]
pub struct BubbleFillResult {
    pub image: RgbImage,
    pub remaining_mask: GrayImage,
    pub filled_pixels: u32,
}

impl BubbleFillResult {
    fn unchanged(image: &RgbImage, mask: &GrayImage) -> Self {
        Self {
            image: image.clone(),
            remaining_mask: mask.clone(),
            filled_pixels: 0,
        }
    }
}

/// Fill masked pixels that fall inside low-variance speech bubbles using the
/// provided bubble-ID mask. Each non-zero pixel value in `bubble_mask`
/// identifies a distinct bubble region.
pub fn apply_bubble_fill(
    image: &RgbImage,
    mask: &GrayImage,
    bubble_mask: &GrayImage,
) -> BubbleFillResult {
    if image.dimensions() != mask.dimensions() || mask.dimensions() != bubble_mask.dimensions() {
        return BubbleFillResult::unchanged(image, mask);
    }

    let bubble_ids = overlapping_bubble_ids(mask, bubble_mask);
    if bubble_ids.is_empty() {
        return BubbleFillResult::unchanged(image, mask);
    }

    let mut result = image.clone();
    let mut remaining_mask = mask.clone();
    let mut filled_pixels = 0u32;

    for bubble_id in bubble_ids {
        let background_mask = bubble_background_mask(&remaining_mask, bubble_mask, bubble_id);
        if count_nonzero(&background_mask) == 0 {
            continue;
        }

        let average_bg_color = match median_rgb(image, &background_mask) {
            Some(color) => color,
            None => continue,
        };
        let std_rgb = color_stddev(image, &background_mask, average_bg_color);
        let inpaint_thresh = if stddev3(std_rgb) > SIMPLE_BG_CHANNEL_STD_SWITCH {
            SIMPLE_BG_THRESHOLD_HIGH_VARIANCE
        } else {
            SIMPLE_BG_THRESHOLD_LOW_VARIANCE
        };
        let std_max = std_rgb.into_iter().fold(0.0, f64::max);
        if std_max >= inpaint_thresh {
            continue;
        }

        let fill = [
            average_bg_color[0] as u8,
            average_bg_color[1] as u8,
            average_bg_color[2] as u8,
        ];

        for y in 0..remaining_mask.height() {
            for x in 0..remaining_mask.width() {
                if remaining_mask.get_pixel(x, y).0[0] == 0 {
                    continue;
                }
                if bubble_mask.get_pixel(x, y).0[0] != bubble_id {
                    continue;
                }
                result.put_pixel(x, y, Rgb(fill));
                remaining_mask.put_pixel(x, y, Luma([0]));
                filled_pixels += 1;
            }
        }
    }

    BubbleFillResult {
        image: result,
        remaining_mask,
        filled_pixels,
    }
}

fn overlapping_bubble_ids(mask: &GrayImage, bubble_mask: &GrayImage) -> Vec<u8> {
    let mut ids = BTreeSet::new();
    for (mask_pixel, bubble_pixel) in mask.pixels().zip(bubble_mask.pixels()) {
        if mask_pixel.0[0] > 0 && bubble_pixel.0[0] > 0 {
            ids.insert(bubble_pixel.0[0]);
        }
    }
    ids.into_iter().collect()
}

fn bubble_background_mask(mask: &GrayImage, bubble_mask: &GrayImage, bubble_id: u8) -> GrayImage {
    let (width, height) = bubble_mask.dimensions();
    let mut out = GrayImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            if bubble_mask.get_pixel(x, y).0[0] == bubble_id && mask.get_pixel(x, y).0[0] == 0 {
                out.put_pixel(x, y, Luma([255]));
            }
        }
    }
    out
}

fn count_nonzero(mask: &GrayImage) -> u32 {
    mask.pixels().filter(|pixel| pixel.0[0] > 0).count() as u32
}

fn median_rgb(image: &RgbImage, mask: &GrayImage) -> Option<[f64; 3]> {
    let mut channels = [Vec::new(), Vec::new(), Vec::new()];
    for (pixel, mask_pixel) in image.pixels().zip(mask.pixels()) {
        if mask_pixel.0[0] == 0 {
            continue;
        }
        channels[0].push(pixel.0[0]);
        channels[1].push(pixel.0[1]);
        channels[2].push(pixel.0[2]);
    }

    Some([
        median_channel(&channels[0])?,
        median_channel(&channels[1])?,
        median_channel(&channels[2])?,
    ])
}

fn median_channel(values: &[u8]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    let mut values = values.to_vec();
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((f64::from(values[mid - 1]) + f64::from(values[mid])) / 2.0)
    } else {
        Some(f64::from(values[mid]))
    }
}

fn color_stddev(image: &RgbImage, mask: &GrayImage, median: [f64; 3]) -> [f64; 3] {
    let mut sum_sq = [0.0; 3];
    let mut count = 0.0;

    for (pixel, mask_pixel) in image.pixels().zip(mask.pixels()) {
        if mask_pixel.0[0] == 0 {
            continue;
        }
        count += 1.0;
        for channel in 0..3 {
            let diff = f64::from(pixel.0[channel]) - median[channel];
            sum_sq[channel] += diff * diff;
        }
    }

    if count == 0.0 {
        return [f64::INFINITY; 3];
    }

    [
        (sum_sq[0] / count).sqrt(),
        (sum_sq[1] / count).sqrt(),
        (sum_sq[2] / count).sqrt(),
    ]
}

fn stddev3(values: [f64; 3]) -> f64 {
    let mean = values.iter().sum::<f64>() / 3.0;
    let variance = values
        .iter()
        .map(|value| {
            let diff = value - mean;
            diff * diff
        })
        .sum::<f64>()
        / 3.0;
    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubble_fill_clears_simple_flat_bubble_pixels() {
        let mut image = RgbImage::from_pixel(64, 64, Rgb([240, 240, 240]));
        let mut mask = GrayImage::new(64, 64);
        let mut bubble_mask = GrayImage::new(64, 64);

        for y in 8..40 {
            for x in 8..56 {
                bubble_mask.put_pixel(x, y, Luma([3]));
            }
        }
        for y in 18..30 {
            for x in 18..46 {
                mask.put_pixel(x, y, Luma([255]));
                image.put_pixel(x, y, Rgb([10, 10, 10]));
            }
        }

        let filled = apply_bubble_fill(&image, &mask, &bubble_mask);

        assert_eq!(filled.filled_pixels, 28 * 12);
        assert_eq!(count_nonzero(&filled.remaining_mask), 0);
        assert_eq!(filled.image.get_pixel(20, 20).0, [240, 240, 240]);
        assert_eq!(filled.image.get_pixel(4, 4).0, [240, 240, 240]);
    }

    #[test]
    fn bubble_fill_skips_textured_bubbles() {
        let mut image = RgbImage::from_pixel(64, 64, Rgb([240, 240, 240]));
        let mut mask = GrayImage::new(64, 64);
        let mut bubble_mask = GrayImage::new(64, 64);

        for y in 8..40 {
            for x in 8..56 {
                bubble_mask.put_pixel(x, y, Luma([5]));
                let stripe = if (x / 3 + y / 2) % 2 == 0 { 35 } else { 245 };
                image.put_pixel(x, y, Rgb([stripe, 255 - stripe, stripe / 2]));
            }
        }
        for y in 18..30 {
            for x in 18..46 {
                mask.put_pixel(x, y, Luma([255]));
                image.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }

        let filled = apply_bubble_fill(&image, &mask, &bubble_mask);

        assert_eq!(filled.filled_pixels, 0);
        assert_eq!(count_nonzero(&filled.remaining_mask), 28 * 12);
        assert_eq!(filled.image.get_pixel(20, 20).0, [0, 0, 0]);
    }

    #[test]
    fn bubble_fill_only_clears_masked_pixels_inside_segmented_bubbles() {
        let mut image = RgbImage::from_pixel(64, 64, Rgb([235, 235, 235]));
        let mut mask = GrayImage::new(64, 64);
        let mut bubble_mask = GrayImage::new(64, 64);

        for y in 10..42 {
            for x in 10..54 {
                bubble_mask.put_pixel(x, y, Luma([7]));
            }
        }
        for y in 20..28 {
            for x in 20..32 {
                mask.put_pixel(x, y, Luma([255]));
                image.put_pixel(x, y, Rgb([5, 5, 5]));
            }
        }
        for y in 44..50 {
            for x in 44..52 {
                mask.put_pixel(x, y, Luma([255]));
                image.put_pixel(x, y, Rgb([5, 5, 5]));
            }
        }

        let filled = apply_bubble_fill(&image, &mask, &bubble_mask);

        assert_eq!(filled.filled_pixels, 12 * 8);
        assert_eq!(count_nonzero(&filled.remaining_mask), 8 * 6);
        assert_eq!(filled.image.get_pixel(22, 22).0, [235, 235, 235]);
        assert_eq!(filled.image.get_pixel(46, 46).0, [5, 5, 5]);
    }
}
