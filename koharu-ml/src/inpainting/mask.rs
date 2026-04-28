use std::collections::VecDeque;

use image::{DynamicImage, GrayImage, Luma, imageops::crop_imm};
use imageproc::{distance_transform::Norm, morphology::dilate};

use crate::{comic_text_detector::expanded_text_block_crop_bounds, types::TextRegion};

use super::{binarize_mask, strategy::boxes_from_mask};

const LEGACY_MIN_DILATE_RADIUS: u8 = 2;
const LEGACY_MAX_DILATE_RADIUS: u8 = 8;
const LEGACY_BLOCK_DILATE_FONT_RATIO: f32 = 0.16;
const LEGACY_COMPONENT_DILATE_RATIO: f32 = 0.35;
const LEGACY_MAX_COMPONENT_DILATE_RADIUS: u8 = 6;

const MODERN_MIN_DILATE_RADIUS: u8 = 3;
const MODERN_MAX_DILATE_RADIUS: u8 = 12;
const MODERN_BLOCK_DILATE_FONT_RATIO: f32 = 0.22;
const MODERN_COMPONENT_DILATE_RATIO: f32 = 0.45;
const MODERN_MAX_COMPONENT_DILATE_RADIUS: u8 = 8;

type Xyxy = [u32; 4];

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExpansionMode {
    LegacyGlyphOnly,
    ModernRegionFill,
}

/// Expand the erase mask before inpainting using text-region geometry and the
/// segmented bubble IDs as hard constraints where available.
///
/// This grows detected glyph pixels only. It must not fill the text block or
/// bubble background, because that turns a text cleanup mask into a broad
/// speech-bubble erase mask.
pub fn expand_mask_for_inpainting(
    mask: &DynamicImage,
    bubble_mask: &DynamicImage,
    text_blocks: &[TextRegion],
) -> GrayImage {
    let base = binarize_mask(mask);
    if base.pixels().all(|pixel| pixel.0[0] == 0) {
        return base;
    }

    let bubbles = bubble_mask.to_luma8();
    if base.dimensions() != bubbles.dimensions() {
        return base;
    }

    let (width, height) = base.dimensions();
    let mut expanded = base.clone();
    let mut covered = GrayImage::new(width, height);

    for block in text_blocks {
        let block_support = expanded_text_block_crop_bounds(width, height, block);
        if count_nonzero_in_rect(&base, block_support) == 0 {
            continue;
        }

        let radius = legacy_block_dilate_radius(block);
        let support = expand_rect(block_support, width, height, u32::from(radius));
        let work = expand_rect(support, width, height, u32::from(radius));
        let local_mask = crop_imm(
            &base,
            work[0],
            work[1],
            work[2] - work[0],
            work[3] - work[1],
        )
        .to_image();
        let dilated = dilate(&local_mask, Norm::LInf, radius);
        let bubble_id = dominant_bubble_id(&base, &bubbles, block_support);
        merge_expanded_region(
            &mut expanded,
            &dilated,
            &bubbles,
            work,
            work, // Fix harsh cutoff by allowing dilation context
            bubble_id,
            Some(&mut covered),
        );
    }

    let residual = GrayImage::from_fn(width, height, |x, y| {
        if base.get_pixel(x, y).0[0] > 0 && covered.get_pixel(x, y).0[0] == 0 {
            Luma([255])
        } else {
            Luma([0])
        }
    });
    if residual.pixels().any(|pixel| pixel.0[0] > 0) {
        expand_residual_components(
            &mut expanded,
            &residual,
            &bubbles,
            ExpansionMode::LegacyGlyphOnly,
        );
    }

    expanded
}

/// Expand the mask to the detected text region, constrained to the dominant
/// speech bubble when one is available. This intentionally keeps the broader
/// 0.48.0 region-fill behavior for Flux.2, while [`expand_mask_for_inpainting`]
/// remains glyph-only for AOT/Lama.
pub fn expand_mask_to_bubble_region_for_inpainting(
    mask: &DynamicImage,
    bubble_mask: &DynamicImage,
    text_blocks: &[TextRegion],
) -> GrayImage {
    let base = binarize_mask(mask);
    if base.pixels().all(|pixel| pixel.0[0] == 0) {
        return base;
    }

    let bubbles = bubble_mask.to_luma8();
    if base.dimensions() != bubbles.dimensions() {
        return base;
    }

    let (width, height) = base.dimensions();
    let mut expanded = base.clone();
    let mut covered = GrayImage::new(width, height);

    for block in text_blocks {
        let block_support = expanded_text_block_crop_bounds(width, height, block);
        let radius = modern_block_dilate_radius(block);
        let support = expand_rect(block_support, width, height, u32::from(radius));

        if count_nonzero_in_rect(&base, support) == 0 {
            continue;
        }

        let bubble_id = dominant_bubble_id(&base, &bubbles, support);
        fill_text_block_region(&mut expanded, &bubbles, support, bubble_id, &mut covered);
    }

    let residual = GrayImage::from_fn(width, height, |x, y| {
        if base.get_pixel(x, y).0[0] > 0 && covered.get_pixel(x, y).0[0] == 0 {
            Luma([255])
        } else {
            Luma([0])
        }
    });
    if residual.pixels().any(|pixel| pixel.0[0] > 0) {
        expand_residual_components(
            &mut expanded,
            &residual,
            &bubbles,
            ExpansionMode::ModernRegionFill,
        );
    }

    expanded
}

fn expand_residual_components(
    out: &mut GrayImage,
    residual: &GrayImage,
    bubbles: &GrayImage,
    mode: ExpansionMode,
) {
    let (width, height) = residual.dimensions();
    for component in boxes_from_mask(residual) {
        let radius = match mode {
            ExpansionMode::LegacyGlyphOnly => legacy_component_dilate_radius(component),
            ExpansionMode::ModernRegionFill => modern_component_dilate_radius(component),
        };
        let support = expand_rect(component, width, height, u32::from(radius));
        let work = expand_rect(support, width, height, u32::from(radius));
        let local_mask = crop_imm(
            residual,
            work[0],
            work[1],
            work[2] - work[0],
            work[3] - work[1],
        )
        .to_image();
        let dilated = dilate(&local_mask, Norm::LInf, radius);
        let bubble_id = dominant_bubble_id(residual, bubbles, support);

        match mode {
            ExpansionMode::LegacyGlyphOnly => {
                merge_expanded_region(out, &dilated, bubbles, work, work, bubble_id, None);
            }
            ExpansionMode::ModernRegionFill => {
                let filled = fill_enclosed_holes(&dilated);
                merge_expanded_region(out, &filled, bubbles, work, support, bubble_id, None);
            }
        }
    }
}

fn fill_text_block_region(
    out: &mut GrayImage,
    bubbles: &GrayImage,
    [x1, y1, x2, y2]: Xyxy,
    bubble_id: u8,
    covered: &mut GrayImage,
) {
    for y in y1..y2 {
        for x in x1..x2 {
            if bubble_id > 0 && bubbles.get_pixel(x, y).0[0] != bubble_id {
                continue;
            }
            out.put_pixel(x, y, Luma([255]));
            covered.put_pixel(x, y, Luma([255]));
        }
    }
}

fn fill_enclosed_holes(mask: &GrayImage) -> GrayImage {
    let (width, height) = mask.dimensions();
    if width == 0 || height == 0 {
        return mask.clone();
    }

    let mut reachable = vec![false; width as usize * height as usize];
    let mut queue = VecDeque::new();

    for x in 0..width {
        enqueue_zero(mask, &mut reachable, &mut queue, x, 0);
        enqueue_zero(mask, &mut reachable, &mut queue, x, height - 1);
    }
    for y in 0..height {
        enqueue_zero(mask, &mut reachable, &mut queue, 0, y);
        enqueue_zero(mask, &mut reachable, &mut queue, width - 1, y);
    }

    while let Some((x, y)) = queue.pop_front() {
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    continue;
                }
                enqueue_zero(mask, &mut reachable, &mut queue, nx as u32, ny as u32);
            }
        }
    }

    GrayImage::from_fn(width, height, |x, y| {
        let index = y as usize * width as usize + x as usize;
        if mask.get_pixel(x, y).0[0] > 0 || !reachable[index] {
            Luma([255])
        } else {
            Luma([0])
        }
    })
}

fn enqueue_zero(
    mask: &GrayImage,
    reachable: &mut [bool],
    queue: &mut VecDeque<(u32, u32)>,
    x: u32,
    y: u32,
) {
    if mask.get_pixel(x, y).0[0] > 0 {
        return;
    }
    let index = y as usize * mask.width() as usize + x as usize;
    if reachable[index] {
        return;
    }
    reachable[index] = true;
    queue.push_back((x, y));
}

fn merge_expanded_region(
    out: &mut GrayImage,
    dilated: &GrayImage,
    bubbles: &GrayImage,
    work: Xyxy,
    support: Xyxy,
    bubble_id: u8,
    mut covered: Option<&mut GrayImage>,
) {
    for local_y in 0..dilated.height() {
        let y = work[1] + local_y;
        if y < support[1] || y >= support[3] {
            continue;
        }
        for local_x in 0..dilated.width() {
            if dilated.get_pixel(local_x, local_y).0[0] == 0 {
                continue;
            }
            let x = work[0] + local_x;
            if x < support[0] || x >= support[2] {
                continue;
            }
            if bubble_id > 0 && bubbles.get_pixel(x, y).0[0] != bubble_id {
                continue;
            }
            out.put_pixel(x, y, Luma([255]));
            if let Some(covered) = covered.as_deref_mut() {
                covered.put_pixel(x, y, Luma([255]));
            }
        }
    }
}

fn count_nonzero_in_rect(mask: &GrayImage, [x1, y1, x2, y2]: Xyxy) -> u32 {
    let mut count = 0;
    for y in y1..y2 {
        for x in x1..x2 {
            if mask.get_pixel(x, y).0[0] > 0 {
                count += 1;
            }
        }
    }
    count
}

fn dominant_bubble_id(mask: &GrayImage, bubbles: &GrayImage, [x1, y1, x2, y2]: Xyxy) -> u8 {
    let mut counts = [0u32; 256];
    for y in y1..y2 {
        for x in x1..x2 {
            if mask.get_pixel(x, y).0[0] == 0 {
                continue;
            }
            let bubble_id = bubbles.get_pixel(x, y).0[0];
            if bubble_id > 0 {
                counts[bubble_id as usize] += 1;
            }
        }
    }

    counts
        .iter()
        .enumerate()
        .skip(1)
        .max_by_key(|(_, count)| *count)
        .and_then(|(id, count)| (*count > 0).then_some(id as u8))
        .unwrap_or(0)
}

fn legacy_block_dilate_radius(block: &TextRegion) -> u8 {
    let font = block
        .detected_font_size_px
        .unwrap_or_else(|| block.width.min(block.height).max(1.0));
    ((font * LEGACY_BLOCK_DILATE_FONT_RATIO).round() as u8)
        .clamp(LEGACY_MIN_DILATE_RADIUS, LEGACY_MAX_DILATE_RADIUS)
}

fn modern_block_dilate_radius(block: &TextRegion) -> u8 {
    let font = block
        .detected_font_size_px
        .unwrap_or_else(|| block.width.min(block.height).max(1.0));
    ((font * MODERN_BLOCK_DILATE_FONT_RATIO).round() as u8)
        .clamp(MODERN_MIN_DILATE_RADIUS, MODERN_MAX_DILATE_RADIUS)
}

fn legacy_component_dilate_radius([x1, y1, x2, y2]: Xyxy) -> u8 {
    let short_side = (x2 - x1).min(y2 - y1).max(1);
    ((short_side as f32 * LEGACY_COMPONENT_DILATE_RATIO).round() as u8)
        .clamp(LEGACY_MIN_DILATE_RADIUS, LEGACY_MAX_COMPONENT_DILATE_RADIUS)
}

fn modern_component_dilate_radius([x1, y1, x2, y2]: Xyxy) -> u8 {
    let short_side = (x2 - x1).min(y2 - y1).max(1);
    ((short_side as f32 * MODERN_COMPONENT_DILATE_RATIO).round() as u8)
        .clamp(MODERN_MIN_DILATE_RADIUS, MODERN_MAX_COMPONENT_DILATE_RADIUS)
}

fn expand_rect([x1, y1, x2, y2]: Xyxy, width: u32, height: u32, pad: u32) -> Xyxy {
    [
        x1.saturating_sub(pad),
        y1.saturating_sub(pad),
        x2.saturating_add(pad).min(width),
        y2.saturating_add(pad).min(height),
    ]
}

#[cfg(test)]
mod tests {
    use image::Luma;

    use super::*;

    #[test]
    fn text_block_expansion_grows_mask_inside_same_bubble() {
        let mut mask = GrayImage::new(64, 64);
        let mut bubbles = GrayImage::new(64, 64);
        for y in 8..56 {
            for x in 8..56 {
                bubbles.put_pixel(x, y, Luma([4]));
            }
        }
        for y in 26..30 {
            for x in 24..40 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let expanded = expand_mask_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[TextRegion {
                x: 22.0,
                y: 24.0,
                width: 20.0,
                height: 10.0,
                detected_font_size_px: Some(18.0),
                ..TextRegion::default()
            }],
        );

        assert_eq!(expanded.get_pixel(24, 24).0[0], 255);
        assert_eq!(expanded.get_pixel(40, 31).0[0], 255);
        assert_eq!(expanded.get_pixel(6, 24).0[0], 0);
    }

    #[test]
    fn text_block_expansion_does_not_fill_text_block_background() {
        let mut mask = GrayImage::new(64, 64);
        let mut bubbles = GrayImage::new(64, 64);
        for y in 8..56 {
            for x in 8..56 {
                bubbles.put_pixel(x, y, Luma([7]));
            }
        }
        for y in 26..28 {
            for x in 24..28 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let expanded = expand_mask_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[TextRegion {
                x: 22.0,
                y: 24.0,
                width: 20.0,
                height: 10.0,
                detected_font_size_px: Some(18.0),
                ..TextRegion::default()
            }],
        );

        assert_eq!(expanded.get_pixel(22, 24).0[0], 255);
        assert_eq!(expanded.get_pixel(40, 31).0[0], 0);
        assert_eq!(expanded.get_pixel(15, 24).0[0], 0);
        assert_eq!(expanded.get_pixel(7, 24).0[0], 0);
    }

    #[test]
    fn text_block_expansion_does_not_fill_bubble_background() {
        let mut mask = GrayImage::new(64, 64);
        let mut bubbles = GrayImage::new(64, 64);
        for y in 8..56 {
            for x in 8..56 {
                bubbles.put_pixel(x, y, Luma([7]));
            }
        }
        for y in 26..28 {
            for x in 24..28 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let expanded = expand_mask_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[TextRegion {
                x: 8.0,
                y: 8.0,
                width: 48.0,
                height: 48.0,
                detected_font_size_px: Some(18.0),
                ..TextRegion::default()
            }],
        );

        assert_eq!(expanded.get_pixel(24, 26).0[0], 255);
        assert_eq!(expanded.get_pixel(10, 10).0[0], 0);
        assert_eq!(expanded.get_pixel(50, 50).0[0], 0);
    }

    #[test]
    fn bubble_region_expansion_fills_text_block_background() {
        let mut mask = GrayImage::new(64, 64);
        let mut bubbles = GrayImage::new(64, 64);
        for y in 8..56 {
            for x in 8..56 {
                bubbles.put_pixel(x, y, Luma([7]));
            }
        }
        for y in 26..28 {
            for x in 24..28 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let expanded = expand_mask_to_bubble_region_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[TextRegion {
                x: 22.0,
                y: 24.0,
                width: 20.0,
                height: 10.0,
                detected_font_size_px: Some(18.0),
                ..TextRegion::default()
            }],
        );

        assert_eq!(expanded.get_pixel(40, 31).0[0], 255);
        assert_eq!(expanded.get_pixel(18, 20).0[0], 255);
        assert_eq!(expanded.get_pixel(7, 24).0[0], 0);
    }

    #[test]
    fn text_block_expansion_stays_inside_dominant_bubble() {
        let mut mask = GrayImage::new(64, 64);
        let mut bubbles = GrayImage::new(64, 64);
        for y in 8..56 {
            for x in 8..32 {
                bubbles.put_pixel(x, y, Luma([3]));
            }
            for x in 32..56 {
                bubbles.put_pixel(x, y, Luma([4]));
            }
        }
        for y in 26..28 {
            for x in 24..28 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let expanded = expand_mask_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[TextRegion {
                x: 22.0,
                y: 24.0,
                width: 20.0,
                height: 10.0,
                detected_font_size_px: Some(18.0),
                ..TextRegion::default()
            }],
        );

        assert_eq!(expanded.get_pixel(30, 28).0[0], 255);
        assert_eq!(expanded.get_pixel(36, 28).0[0], 0);
    }

    #[test]
    fn bubble_region_expansion_fills_enclosed_holes() {
        let mut mask = GrayImage::new(64, 64);
        let mut bubbles = GrayImage::new(64, 64);
        for y in 8..56 {
            for x in 8..56 {
                bubbles.put_pixel(x, y, Luma([7]));
            }
        }
        for y in 20..44 {
            for x in 20..44 {
                if !(26..38).contains(&x) || !(26..38).contains(&y) {
                    mask.put_pixel(x, y, Luma([255]));
                }
            }
        }

        // Test modern path via public API with empty text_blocks to trigger residual components logic
        let expanded = expand_mask_to_bubble_region_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[],
        );

        assert_eq!(expanded.get_pixel(32, 32).0[0], 255);
    }

    #[test]
    fn bubble_region_expansion_stays_inside_dominant_bubble() {
        let mut mask = GrayImage::new(64, 64);
        let mut bubbles = GrayImage::new(64, 64);
        for y in 8..56 {
            for x in 8..32 {
                bubbles.put_pixel(x, y, Luma([3]));
            }
            for x in 32..56 {
                bubbles.put_pixel(x, y, Luma([4]));
            }
        }
        for y in 26..28 {
            for x in 24..28 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let expanded = expand_mask_to_bubble_region_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[TextRegion {
                x: 22.0,
                y: 24.0,
                width: 20.0,
                height: 10.0,
                detected_font_size_px: Some(18.0),
                ..TextRegion::default()
            }],
        );

        assert_eq!(expanded.get_pixel(30, 28).0[0], 255);
        assert_eq!(expanded.get_pixel(36, 28).0[0], 0);
    }

    #[test]
    fn mask_expansion_does_not_fill_text_block_background_for_sparse_glyphs() {
        let mut mask = GrayImage::new(64, 64);
        let mut bubbles = GrayImage::new(64, 64);
        for y in 8..56 {
            for x in 8..56 {
                bubbles.put_pixel(x, y, Luma([7]));
            }
        }
        for y in 26..28 {
            for x in 24..28 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let expanded = expand_mask_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[TextRegion {
                x: 22.0,
                y: 24.0,
                width: 20.0,
                height: 10.0,
                detected_font_size_px: Some(18.0),
                ..TextRegion::default()
            }],
        );

        assert_eq!(expanded.get_pixel(24, 26).0[0], 255);
        assert_eq!(expanded.get_pixel(22, 24).0[0], 255);
        assert_eq!(expanded.get_pixel(40, 31).0[0], 0);
    }

    #[test]
    fn residual_component_expansion_still_works_without_text_blocks() {
        let mut mask = GrayImage::new(48, 48);
        let bubbles = GrayImage::new(48, 48);
        for y in 20..24 {
            for x in 20..24 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }

        let expanded = expand_mask_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[],
        );

        assert_eq!(expanded.get_pixel(18, 22).0[0], 255);
        assert_eq!(expanded.get_pixel(22, 18).0[0], 255);
        assert_eq!(expanded.get_pixel(10, 10).0[0], 0);
    }

    #[test]
    fn text_block_expansion_bleeds_out_of_support() {
        let mut mask = GrayImage::new(64, 64);
        let mut bubbles = GrayImage::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                bubbles.put_pixel(x, y, Luma([1]));
            }
        }
        // Place a single pixel at the edge of the block
        mask.put_pixel(29, 29, Luma([255]));

        let block = TextRegion {
            x: 20.0,
            y: 20.0,
            width: 10.0,  // Right edge is at 30
            height: 10.0, // Bottom edge is at 30
            detected_font_size_px: Some(20.0),
            ..Default::default()
        };

        let expanded = expand_mask_for_inpainting(
            &DynamicImage::ImageLuma8(mask),
            &DynamicImage::ImageLuma8(bubbles),
            &[block],
        );

        // Radius is (20 * 0.16) = 3.2 -> 3.
        // (29+3, 29) = (32, 29)
        assert_eq!(expanded.get_pixel(32, 29).0[0], 255);
        // (33, 29) should be 0
        assert_eq!(expanded.get_pixel(33, 29).0[0], 0);
    }
}
