use crate::types::{TextDirection, TextRegion};
use image::{
    DynamicImage, GrayImage, Luma, Rgb, RgbImage,
    imageops::{self},
};
use imageproc::{
    distance_transform::Norm,
    geometric_transformations::{Interpolation, Projection, warp_into},
    morphology::dilate,
};

const FINAL_MASK_DILATE_RADIUS: u8 = 2;

pub type Quad = [[f32; 2]; 4];

#[derive(Debug, Clone)]
pub struct ComicTextDetection {
    pub shrink_map: GrayImage,
    pub threshold_map: GrayImage,
    pub line_polygons: Vec<Quad>,
    pub text_blocks: Vec<TextRegion>,
    pub mask: GrayImage,
}

pub fn refine_segmentation_mask(
    _image: &DynamicImage,
    pred_mask: &GrayImage,
    blocks: &[TextRegion],
) -> GrayImage {
    let width = pred_mask.width();
    let height = pred_mask.height();

    if blocks.is_empty() {
        return GrayImage::new(width, height);
    }

    // Extract expanded bounding boxes globally to validate intersection constraints.
    let expanded_bounds: Vec<[u32; 4]> = blocks
        .iter()
        .map(|b| expanded_text_block_crop_bounds(width, height, b))
        .collect();

    // Rasterize the union of expanded text block bounds once to avoid an
    // O(width * height * blocks) per-pixel rectangle membership test.
    let mut in_bounds_mask = GrayImage::new(width, height);
    for &[x1, y1, x2, y2] in &expanded_bounds {
        for y in y1..y2 {
            for x in x1..x2 {
                in_bounds_mask.put_pixel(x, y, Luma([255]));
            }
        }
    }

    // Apply a threshold mask: Pixels are preserved exclusively if their probability
    // exceeds the core threshold (`super::BINARY_THRESHOLD`) and they reside within a known TextRegion geometry.
    let base = GrayImage::from_fn(width, height, |x, y| {
        if in_bounds_mask.get_pixel(x, y)[0] != 0
            && pred_mask.get_pixel(x, y)[0] > super::BINARY_THRESHOLD
        {
            Luma([255])
        } else {
            Luma([0])
        }
    });

    let dilated = dilate(&base, Norm::L1, FINAL_MASK_DILATE_RADIUS);

    // Final clipping pass: Ensure the dilated mask never escapes the block boundaries
    // even if it thickens beyond its original source pixel edges.
    GrayImage::from_fn(width, height, |x, y| {
        if in_bounds_mask.get_pixel(x, y)[0] != 0 {
            *dilated.get_pixel(x, y)
        } else {
            Luma([0])
        }
    })
}

pub fn crop_text_block_bbox(image: &DynamicImage, block: &TextRegion) -> DynamicImage {
    let [x1, y1, x2, y2] = expanded_text_block_crop_bounds(image.width(), image.height(), block);
    image.crop_imm(x1, y1, x2.saturating_sub(x1), y2.saturating_sub(y1))
}

pub fn extract_text_block_regions(image: &DynamicImage, block: &TextRegion) -> Vec<DynamicImage> {
    let Some(line_polygons) = block.line_polygons.as_ref() else {
        return vec![crop_text_block_bbox(image, block)];
    };
    if line_polygons.is_empty() {
        return vec![crop_text_block_bbox(image, block)];
    }

    let rgb = image.to_rgb8();
    let mut regions = Vec::with_capacity(line_polygons.len());
    for line in line_polygons {
        if let Some(region) = warp_line_region(&rgb, block, line) {
            regions.push(DynamicImage::ImageRgb8(region));
        }
    }

    if regions.is_empty() {
        vec![crop_text_block_bbox(image, block)]
    } else {
        regions
    }
}

fn expanded_text_block_crop_bounds(
    image_width: u32,
    image_height: u32,
    block: &TextRegion,
) -> [u32; 4] {
    let should_expand = block.detector.as_deref() == Some("ctd")
        || block
            .line_polygons
            .as_ref()
            .map(|lines| !lines.is_empty())
            .unwrap_or(false);
    if !should_expand {
        let x1 = block.x.max(0.0).floor() as u32;
        let y1 = block.y.max(0.0).floor() as u32;
        let x2 = (block.x + block.width)
            .ceil()
            .clamp(x1 as f32 + 1.0, image_width as f32) as u32;
        let y2 = (block.y + block.height)
            .ceil()
            .clamp(y1 as f32 + 1.0, image_height as f32) as u32;
        return [x1, y1, x2, y2];
    }

    let mut min_x = block.x;
    let mut min_y = block.y;
    let mut max_x = block.x + block.width;
    let mut max_y = block.y + block.height;

    if let Some(line_polygons) = block.line_polygons.as_ref() {
        for line in line_polygons {
            let quad = maybe_expand_ctd_line(block, line);
            let bbox = quad_bbox(&quad);
            min_x = min_x.min(bbox[0]);
            min_y = min_y.min(bbox[1]);
            max_x = max_x.max(bbox[2]);
            max_y = max_y.max(bbox[3]);
        }
    }

    let font = block
        .detected_font_size_px
        .unwrap_or_else(|| block.width.min(block.height).max(1.0));
    let base_pad = (font * 0.08).max(2.0);
    let (pad_x, pad_y) = match block.source_direction.unwrap_or(TextDirection::Horizontal) {
        TextDirection::Horizontal => ((font * 0.12).max(base_pad), (font * 0.18).max(base_pad)),
        TextDirection::Vertical => ((font * 0.18).max(base_pad), (font * 0.12).max(base_pad)),
    };

    let x1 = (min_x - pad_x)
        .floor()
        .clamp(0.0, image_width.saturating_sub(1) as f32) as u32;
    let y1 = (min_y - pad_y)
        .floor()
        .clamp(0.0, image_height.saturating_sub(1) as f32) as u32;
    let x2 = (max_x + pad_x)
        .ceil()
        .clamp(x1 as f32 + 1.0, image_width as f32) as u32;
    let y2 = (max_y + pad_y)
        .ceil()
        .clamp(y1 as f32 + 1.0, image_height as f32) as u32;
    [x1, y1, x2, y2]
}

fn warp_line_region(image: &RgbImage, block: &TextRegion, line: &Quad) -> Option<RgbImage> {
    let expanded = maybe_expand_ctd_line(block, line);
    let clipped = clip_quad(&expanded, image.width() as f32, image.height() as f32);
    let bbox = quad_bbox(&clipped);
    let x1 = bbox[0].floor().max(0.0) as u32;
    let y1 = bbox[1].floor().max(0.0) as u32;
    let x2 = bbox[2].ceil().min(image.width() as f32) as u32;
    let y2 = bbox[3].ceil().min(image.height() as f32) as u32;
    if x2 <= x1 || y2 <= y1 {
        return None;
    }

    let cropped = imageops::crop_imm(image, x1, y1, x2 - x1, y2 - y1).to_image();
    let mut src = clipped;
    for point in &mut src {
        point[0] -= x1 as f32;
        point[1] -= y1 as f32;
    }

    let (norm_v, norm_h) = quad_axis_lengths(&src);
    if norm_v <= 0.0 || norm_h <= 0.0 {
        return None;
    }

    let direction = block.source_direction.unwrap_or(TextDirection::Horizontal);
    let text_height = match direction {
        TextDirection::Horizontal => norm_v.max(1.0).round() as u32,
        TextDirection::Vertical => norm_h.max(1.0).round() as u32,
    }
    .max(1);
    let ratio = norm_v / norm_h;

    let (width, height, rotate_vertical) = match direction {
        TextDirection::Horizontal => {
            let h = text_height.max(1);
            let w = ((text_height as f32 / ratio).round() as u32).max(1);
            (w, h, false)
        }
        TextDirection::Vertical => {
            let w = text_height.max(1);
            let h = ((text_height as f32 * ratio).round() as u32).max(1);
            (w, h, true)
        }
    };

    let dst = [
        (0.0f32, 0.0f32),
        ((width.saturating_sub(1)) as f32, 0.0f32),
        (
            (width.saturating_sub(1)) as f32,
            (height.saturating_sub(1)) as f32,
        ),
        (0.0f32, (height.saturating_sub(1)) as f32),
    ];
    let src = quad_to_tuples(&src);
    let projection = Projection::from_control_points(src, dst)?;

    let mut region = RgbImage::from_pixel(width, height, Rgb([0, 0, 0]));
    warp_into(
        &cropped,
        &projection,
        Interpolation::Bilinear,
        Rgb([0, 0, 0]),
        &mut region,
    );

    if rotate_vertical {
        Some(imageops::rotate270(&region))
    } else {
        Some(region)
    }
}

fn maybe_expand_ctd_line(block: &TextRegion, line: &Quad) -> Quad {
    let should_expand = block.detector.as_deref() == Some("ctd")
        && block.source_direction == Some(TextDirection::Horizontal);
    if !should_expand {
        return *line;
    }

    let expand_size = (block.detected_font_size_px.unwrap_or(0.0) * 0.1).max(3.0);
    let angle = block.rotation_deg.unwrap_or(0.0).to_radians();
    let sin = angle.sin();
    let cos = angle.cos();
    let signs = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];

    let mut out = *line;
    for (index, point) in out.iter_mut().enumerate() {
        point[0] += signs[index][0] * sin * expand_size;
        point[1] += signs[index][1] * cos * expand_size;
    }
    out
}

fn clip_quad(quad: &Quad, width: f32, height: f32) -> Quad {
    let mut clipped = *quad;
    for point in &mut clipped {
        point[0] = point[0].clamp(0.0, width);
        point[1] = point[1].clamp(0.0, height);
    }
    clipped
}

fn quad_bbox(quad: &Quad) -> [f32; 4] {
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for point in quad {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    [min_x, min_y, max_x, max_y]
}

fn quad_to_tuples(quad: &Quad) -> [(f32, f32); 4] {
    [
        (quad[0][0], quad[0][1]),
        (quad[1][0], quad[1][1]),
        (quad[2][0], quad[2][1]),
        (quad[3][0], quad[3][1]),
    ]
}

fn quad_axis_lengths(quad: &Quad) -> (f32, f32) {
    let midpoints = [
        midpoint(quad[0], quad[1]),
        midpoint(quad[1], quad[2]),
        midpoint(quad[2], quad[3]),
        midpoint(quad[3], quad[0]),
    ];
    let vec_v = [
        midpoints[2][0] - midpoints[0][0],
        midpoints[2][1] - midpoints[0][1],
    ];
    let vec_h = [
        midpoints[1][0] - midpoints[3][0],
        midpoints[1][1] - midpoints[3][1],
    ];
    (vector_norm(vec_v), vector_norm(vec_h))
}

fn midpoint(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

fn vector_norm(vector: [f32; 2]) -> f32 {
    (vector[0] * vector[0] + vector[1] * vector[1]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refine_segmentation_mask_erases_when_blocks_are_missing() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(16, 16, Rgb([255, 255, 255])));
        let pred_mask = GrayImage::from_fn(16, 16, |x, y| {
            if (4..12).contains(&x) && (5..11).contains(&y) {
                Luma([200])
            } else {
                Luma([0])
            }
        });

        let mask = refine_segmentation_mask(&image, &pred_mask, &[]);
        assert_eq!(mask.get_pixel(0, 0)[0], 0);
        assert_eq!(mask.get_pixel(8, 8)[0], 0); // No blocks, must be wiped cleanly
    }

    #[test]
    fn refine_segmentation_mask_clips_outside_blocks() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(32, 32, Rgb([255, 255, 255])));
        let pred_mask = GrayImage::from_fn(32, 32, |x, y| {
            if (8..24).contains(&x) && (10..22).contains(&y) {
                Luma([200])
            } else {
                Luma([0])
            }
        });

        let block = TextRegion {
            x: 10.0,
            y: 11.0,
            width: 4.0, // Limits to roughly [10, 11] to [14, 15]
            height: 4.0,
            detected_font_size_px: Some(4.0),
            ..Default::default()
        };

        let mask = refine_segmentation_mask(&image, &pred_mask, &[block]);
        let without_blocks = refine_segmentation_mask(&image, &pred_mask, &[]);

        // Assert providing bounding blocks saves the mask within bounds
        assert_ne!(mask, without_blocks);
        // Assert pixel INSIDE the block is preserved
        assert_eq!(mask.get_pixel(12, 13)[0], 255);
        // Assert pixel OUTSIDE the block (but inside high-prob region) is cleared
        assert_eq!(mask.get_pixel(20, 13)[0], 0);
        // Assert pixel JUST OUTSIDE the block boundary is cleared
        assert_eq!(mask.get_pixel(15, 13)[0], 0);
    }

    #[test]
    fn extract_text_block_regions_falls_back_to_bbox_without_lines() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(24, 24, Rgb([255, 255, 255])));
        let block = TextRegion {
            x: 4.0,
            y: 5.0,
            width: 10.0,
            height: 8.0,
            ..Default::default()
        };

        let regions = extract_text_block_regions(&image, &block);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].width(), 10);
        assert_eq!(regions[0].height(), 8);
    }

    #[test]
    fn crop_text_block_bbox_expands_ctd_crop() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(48, 48, Rgb([255, 255, 255])));
        let block = TextRegion {
            x: 10.0,
            y: 12.0,
            width: 12.0,
            height: 8.0,
            line_polygons: Some(vec![[
                [10.0, 12.0],
                [22.0, 12.0],
                [22.0, 20.0],
                [10.0, 20.0],
            ]]),
            source_direction: Some(TextDirection::Horizontal),
            rotation_deg: Some(0.0),
            detected_font_size_px: Some(8.0),
            detector: Some("ctd".to_string()),
            ..Default::default()
        };

        let crop = crop_text_block_bbox(&image, &block);
        assert!(crop.width() > 12);
        assert!(crop.height() > 8);
    }
}
