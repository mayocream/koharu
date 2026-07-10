use std::collections::VecDeque;

use anyhow::{Context, Result, bail};
use image::{DynamicImage, GenericImageView, GrayImage, Luma, RgbImage, imageops::crop_imm};
use imageproc::{contours::find_contours_with_threshold, contrast::otsu_level, point::Point};
use koharu_torch::{Device, Kind, Tensor};
use serde::Serialize;

use super::model::Output;

const DBNET_BINARY_THRESHOLD: u8 = 76;
const LINE_SCORE_THRESHOLD: f32 = 0.6;
const MAX_LINE_CANDIDATES: usize = 1000;
const LINE_UNCLIP_RATIO: f32 = 1.5;
const MASK_SCORE_THRESHOLD: f32 = 0.1;

pub type Quad = [[f32; 2]; 4];

#[derive(Debug)]
pub struct PreprocessedImage {
    pub pixel_values: Tensor,
    original_width: u32,
    original_height: u32,
    resized_width: u32,
    resized_height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicTextBlock {
    pub bbox: [f32; 4],
    pub score: f32,
    pub class_id: usize,
    pub label: String,
    pub line_polygons: Vec<Quad>,
    pub vertical: bool,
    pub rotation_deg: f32,
    pub detected_font_size: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicTextDetection {
    pub image_width: u32,
    pub image_height: u32,
    pub blocks: Vec<ComicTextBlock>,
    pub line_polygons: Vec<Quad>,
    #[serde(skip_serializing)]
    pub mask: GrayImage,
    #[serde(skip_serializing)]
    pub shrink_map: GrayImage,
    #[serde(skip_serializing)]
    pub threshold_map: GrayImage,
}

pub fn preprocess(image: &DynamicImage, device: Device) -> Result<PreprocessedImage> {
    let (original_width, original_height) = image.dimensions();
    if original_width == 0 || original_height == 0 {
        bail!("empty image");
    }

    let scale = (1280.0 / original_width as f32).min(1280.0 / original_height as f32);
    let resized_width = ((original_width as f32 * scale).round() as u32).max(1);
    let resized_height = ((original_height as f32 * scale).round() as u32).max(1);
    let pixel_values = image_to_tensor(image, device)?
        .upsample_bilinear2d(
            [resized_height as i64, resized_width as i64],
            false,
            None::<f64>,
            None::<f64>,
        )
        .constant_pad_nd([
            0,
            1280 - resized_width as i64,
            0,
            1280 - resized_height as i64,
        ]);

    Ok(PreprocessedImage {
        pixel_values,
        original_width,
        original_height,
        resized_width,
        resized_height,
    })
}

pub fn postprocess(
    outputs: Output,
    input: &PreprocessedImage,
    source: &DynamicImage,
) -> Result<ComicTextDetection> {
    let maps = Tensor::cat(&[outputs.mask, outputs.line_maps], 1)
        .narrow(2, 0, input.resized_height as i64)
        .narrow(3, 0, input.resized_width as i64);
    let original_maps = maps.upsample_bilinear2d(
        [input.original_height as i64, input.original_width as i64],
        false,
        None::<f64>,
        None::<f64>,
    );
    let packed = Tensor::cat(
        &[
            original_maps.view([-1]),
            maps.narrow(1, 1, 1).contiguous().view([-1]),
        ],
        0,
    );
    let values = tensor_to_u8_vec(packed)?;
    let original_plane = input.original_width as usize * input.original_height as usize;
    let resized_plane = input.resized_width as usize * input.resized_height as usize;
    let raw_mask = gray_from_slice(
        input.original_width,
        input.original_height,
        &values[..original_plane],
    )?;
    let shrink_map = gray_from_slice(
        input.original_width,
        input.original_height,
        &values[original_plane..2 * original_plane],
    )?;
    let threshold_map = gray_from_slice(
        input.original_width,
        input.original_height,
        &values[2 * original_plane..3 * original_plane],
    )?;
    let shrink = gray_from_slice(
        input.resized_width,
        input.resized_height,
        &values[3 * original_plane..3 * original_plane + resized_plane],
    )?;

    let lines = extract_line_polygons(
        &shrink,
        input.original_width as f32 / input.resized_width as f32,
        input.original_height as f32 / input.resized_height as f32,
        input.original_width,
        input.original_height,
    );
    finalize_detection(source, raw_mask, shrink_map, threshold_map, lines)
}

pub fn rearranged_inference<F>(
    source: &DynamicImage,
    device: Device,
    forward: F,
) -> Result<Option<ComicTextDetection>>
where
    F: Fn(&Tensor) -> Output,
{
    let (source_width, source_height) = source.dimensions();
    if source_width == 0 || source_height == 0 {
        bail!("empty image");
    }

    let transposed = source_height < source_width;
    let source_tensor = image_to_tensor(source, device)?;
    let oriented = if transposed {
        source_tensor.transpose(2, 3).contiguous()
    } else {
        source_tensor
    };
    let (width, height) = if transposed {
        (source_height, source_width)
    } else {
        (source_width, source_height)
    };
    let aspect_ratio = height as f32 / width as f32;
    let downscale_ratio = height as f32 / 1280.0;
    if downscale_ratio <= 2.5 || aspect_ratio <= 3.0 {
        return Ok(None);
    }

    let strips_per_composite = ((2 * 1280) / width).clamp(2, 1280) as usize;
    let patch_height = width
        .checked_mul(strips_per_composite as u32)
        .context("comic text detector rearranged patch is too large")?;
    let patch_count = height.div_ceil(patch_height) as usize;
    let patch_starts = (0..patch_count)
        .map(|index| {
            if patch_count == 1 {
                0
            } else {
                (((height - patch_height) as f64 * index as f64) / (patch_count - 1) as f64).round()
                    as u32
            }
        })
        .collect::<Vec<_>>();
    let composite_count = patch_count.div_ceil(strips_per_composite);
    let map_sum = Tensor::zeros([1, 3, height as i64, width as i64], (Kind::Float, device));
    let sample_count = Tensor::zeros([1, 1, height as i64, width as i64], (Kind::Float, device));

    for first_composite in (0..composite_count).step_by(4) {
        let batch_len = 4.min(composite_count - first_composite);
        let tensors = (0..batch_len)
            .map(|offset| {
                let composite_index = first_composite + offset;
                make_rearranged_composite(
                    &oriented,
                    &patch_starts,
                    composite_index,
                    strips_per_composite,
                    patch_height,
                    1280,
                )
            })
            .collect::<Vec<_>>();
        let batch = Tensor::cat(&tensors, 0);
        let outputs = forward(&batch);
        let maps = Tensor::cat(&[outputs.mask, outputs.line_maps], 1);
        let map_width = maps.size()[3] as u32;
        let strip_width = map_width / strips_per_composite as u32;

        for local_index in 0..batch_len {
            let composite_index = first_composite + local_index;
            for strip_index in 0..strips_per_composite {
                let patch_index = composite_index * strips_per_composite + strip_index;
                if patch_index >= patch_count {
                    break;
                }
                let left = strip_index as u32 * strip_width;
                let patch = maps
                    .narrow(0, local_index as i64, 1)
                    .narrow(3, left as i64, strip_width as i64)
                    .upsample_bilinear2d(
                        [patch_height as i64, width as i64],
                        false,
                        None::<f64>,
                        None::<f64>,
                    );
                let top = patch_starts[patch_index] as i64;
                let mut sum_view = map_sum.narrow(2, top, patch_height as i64);
                let _ = sum_view.g_add_(&patch);
                let mut count_view = sample_count.narrow(2, top, patch_height as i64);
                let _ = count_view.g_add_scalar_(1.0);
            }
        }
    }

    let maps = map_sum / sample_count.clamp_min(1.0);
    let maps = if transposed {
        maps.transpose(2, 3).contiguous()
    } else {
        maps
    };
    let values = tensor_to_u8_vec(maps.view([-1]))?;
    let plane = source_width as usize * source_height as usize;
    let mask = gray_from_slice(source_width, source_height, &values[..plane])?;
    let shrink = gray_from_slice(source_width, source_height, &values[plane..2 * plane])?;
    let threshold = gray_from_slice(source_width, source_height, &values[2 * plane..3 * plane])?;
    let lines = extract_line_polygons(&shrink, 1.0, 1.0, source_width, source_height);
    finalize_detection(source, mask, shrink, threshold, lines).map(Some)
}

fn finalize_detection(
    source: &DynamicImage,
    raw_mask: GrayImage,
    shrink_map: GrayImage,
    threshold_map: GrayImage,
    lines: Vec<ScoredQuad>,
) -> Result<ComicTextDetection> {
    let blocks = group_text_lines(&lines, &raw_mask);
    let refined = refine_mask(source, &raw_mask, &blocks);
    let mask = dilate_binary(&refined, 2, MorphShape::Ellipse);
    Ok(ComicTextDetection {
        image_width: source.width(),
        image_height: source.height(),
        line_polygons: lines.iter().map(|line| line.quad).collect(),
        blocks,
        mask,
        shrink_map,
        threshold_map,
    })
}

fn image_to_tensor(image: &DynamicImage, device: Device) -> Result<Tensor> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    Ok((Tensor::from_slice(rgb.as_raw())
        .view([height as i64, width as i64, 3])
        .permute([2, 0, 1])
        .unsqueeze(0)
        .to_device(device)
        .to_kind(Kind::Float))
        / 255.0)
}

fn tensor_to_u8_vec(tensor: Tensor) -> Result<Vec<u8>> {
    let tensor = tensor.clamp(0.0, 1.0) * 255.0;
    let tensor = tensor
        .round()
        .to_kind(Kind::Uint8)
        .to_device(Device::Cpu)
        .contiguous()
        .view([-1]);
    Ok(Vec::<u8>::try_from(&tensor)?)
}

fn gray_from_slice(width: u32, height: u32, pixels: &[u8]) -> Result<GrayImage> {
    GrayImage::from_raw(width, height, pixels.to_vec())
        .context("failed to create gray image from comic text detector tensor")
}

fn make_rearranged_composite(
    image: &Tensor,
    patch_starts: &[u32],
    composite_index: usize,
    strips_per_composite: usize,
    patch_height: u32,
    detect_size: u32,
) -> Tensor {
    let target_width = detect_size / strips_per_composite as u32;
    let mut strips = Vec::with_capacity(strips_per_composite);
    for strip_index in 0..strips_per_composite {
        let patch_index = composite_index * strips_per_composite + strip_index;
        let strip = if patch_index < patch_starts.len() {
            image
                .narrow(2, patch_starts[patch_index] as i64, patch_height as i64)
                .upsample_bilinear2d(
                    [detect_size as i64, target_width as i64],
                    false,
                    None::<f64>,
                    None::<f64>,
                )
        } else {
            Tensor::zeros(
                [1, 3, detect_size as i64, target_width as i64],
                (Kind::Float, image.device()),
            )
        };
        strips.push(strip);
    }
    let composite = Tensor::cat(&strips, 3);
    let padding = detect_size as i64 - composite.size()[3];
    if padding > 0 {
        composite.constant_pad_nd([0, padding, 0, 0])
    } else {
        composite
    }
}

#[derive(Clone, Copy)]
struct ScoredQuad {
    quad: Quad,
    score: f32,
}

#[derive(Clone, Copy)]
struct RotatedRect {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    cos: f32,
    sin: f32,
}

impl RotatedRect {
    fn width(self) -> f32 {
        self.max_x - self.min_x
    }

    fn height(self) -> f32 {
        self.max_y - self.min_y
    }

    fn corners(self, expand: f32) -> Quad {
        let points = [
            [self.min_x - expand, self.min_y - expand],
            [self.max_x + expand, self.min_y - expand],
            [self.max_x + expand, self.max_y + expand],
            [self.min_x - expand, self.max_y + expand],
        ];
        points.map(|[x, y]| [x * self.cos - y * self.sin, x * self.sin + y * self.cos])
    }
}

fn extract_line_polygons(
    map: &GrayImage,
    scale_x: f32,
    scale_y: f32,
    dest_width: u32,
    dest_height: u32,
) -> Vec<ScoredQuad> {
    find_contours_with_threshold::<i32>(map, DBNET_BINARY_THRESHOLD)
        .into_iter()
        .take(MAX_LINE_CANDIDATES)
        .filter_map(|contour| {
            let points = contour.points;
            if points.len() < 3 {
                return None;
            }
            let rect = minimum_area_rect(&points)?;
            if rect.width().min(rect.height()) < 2.0 {
                return None;
            }
            let score = polygon_score(map, &points);
            if score <= LINE_SCORE_THRESHOLD {
                return None;
            }
            let perimeter = 2.0 * (rect.width() + rect.height());
            let expand = if perimeter > f32::EPSILON {
                rect.width() * rect.height() * LINE_UNCLIP_RATIO / perimeter
            } else {
                0.0
            };
            let mut quad = rect.corners(expand);
            for point in &mut quad {
                point[0] = (point[0] * scale_x).round().clamp(0.0, dest_width as f32);
                point[1] = (point[1] * scale_y).round().clamp(0.0, dest_height as f32);
            }
            Some(ScoredQuad { quad, score })
        })
        .collect()
}

fn minimum_area_rect(points: &[Point<i32>]) -> Option<RotatedRect> {
    let hull = convex_hull(points);
    if hull.len() < 3 {
        return None;
    }
    let mut best = None;
    let mut best_area = f32::INFINITY;
    for index in 0..hull.len() {
        let a = hull[index];
        let b = hull[(index + 1) % hull.len()];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = dx.hypot(dy);
        if length <= f32::EPSILON {
            continue;
        }
        let cos = dx / length;
        let sin = dy / length;
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for point in &hull {
            let x = point[0] * cos + point[1] * sin;
            let y = -point[0] * sin + point[1] * cos;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        let area = (max_x - min_x) * (max_y - min_y);
        if area < best_area {
            best_area = area;
            best = Some(RotatedRect {
                min_x,
                min_y,
                max_x,
                max_y,
                cos,
                sin,
            });
        }
    }
    best
}

fn convex_hull(points: &[Point<i32>]) -> Vec<[f32; 2]> {
    let mut points = points
        .iter()
        .map(|point| [point.x as f32, point.y as f32])
        .collect::<Vec<_>>();
    points.sort_unstable_by(|a, b| a[0].total_cmp(&b[0]).then_with(|| a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() <= 2 {
        return points;
    }

    let mut lower = Vec::new();
    for point in &points {
        while lower.len() >= 2
            && cross(lower[lower.len() - 2], lower[lower.len() - 1], *point) <= 0.0
        {
            lower.pop();
        }
        lower.push(*point);
    }
    let mut upper = Vec::new();
    for point in points.iter().rev() {
        while upper.len() >= 2
            && cross(upper[upper.len() - 2], upper[upper.len() - 1], *point) <= 0.0
        {
            upper.pop();
        }
        upper.push(*point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn cross(origin: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - origin[0]) * (b[1] - origin[1]) - (a[1] - origin[1]) * (b[0] - origin[0])
}

fn polygon_score(map: &GrayImage, polygon: &[Point<i32>]) -> f32 {
    let min_x = polygon
        .iter()
        .map(|point| point.x)
        .min()
        .unwrap_or(0)
        .max(0) as u32;
    let min_y = polygon
        .iter()
        .map(|point| point.y)
        .min()
        .unwrap_or(0)
        .max(0) as u32;
    let max_x = polygon
        .iter()
        .map(|point| point.x)
        .max()
        .unwrap_or(0)
        .clamp(0, map.width().saturating_sub(1) as i32) as u32;
    let max_y = polygon
        .iter()
        .map(|point| point.y)
        .max()
        .unwrap_or(0)
        .clamp(0, map.height().saturating_sub(1) as i32) as u32;
    let polygon = polygon
        .iter()
        .map(|point| [point.x as f32, point.y as f32])
        .collect::<Vec<_>>();
    let mut sum = 0u64;
    let mut count = 0u64;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if point_in_polygon([x as f32 + 0.5, y as f32 + 0.5], &polygon) {
                sum += map.get_pixel(x, y)[0] as u64;
                count += 1;
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        sum as f32 / count as f32 / 255.0
    }
}

fn point_in_polygon(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        if point_on_segment(point, previous, current) {
            return true;
        }
        if (current[1] > point[1]) != (previous[1] > point[1])
            && point[0]
                < (previous[0] - current[0]) * (point[1] - current[1]) / (previous[1] - current[1])
                    + current[0]
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn point_on_segment(point: [f32; 2], a: [f32; 2], b: [f32; 2]) -> bool {
    cross(a, b, point).abs() < 1e-3
        && point[0] >= a[0].min(b[0])
        && point[0] <= a[0].max(b[0])
        && point[1] >= a[1].min(b[1])
        && point[1] <= a[1].max(b[1])
}

#[derive(Clone)]
struct WorkingBlock {
    bbox: [f32; 4],
    score: f32,
    lines: Vec<Quad>,
    vertical: bool,
    angle: f32,
    font_size: f32,
    vector: [f32; 2],
    norm: f32,
    merged: bool,
}

impl WorkingBlock {
    fn from_line(line: ScoredQuad) -> Self {
        let (quad, vertical) = sort_line_quad(line.quad);
        let mut block = Self {
            bbox: quad_bbox(&quad),
            score: line.score,
            lines: vec![quad],
            vertical,
            angle: 0.0,
            font_size: 0.0,
            vector: [0.0, 0.0],
            norm: 0.0,
            merged: false,
        };
        block.recalculate();
        block
    }

    fn center(&self) -> [f32; 2] {
        [
            (self.bbox[0] + self.bbox[2]) * 0.5,
            (self.bbox[1] + self.bbox[3]) * 0.5,
        ]
    }

    fn recalculate(&mut self) {
        self.bbox = lines_bbox(&self.lines);
        let mut vertical_vector = [0.0f32; 2];
        let mut horizontal_vector = [0.0f32; 2];
        for line in &self.lines {
            let top = midpoint(line[0], line[1]);
            let right = midpoint(line[1], line[2]);
            let bottom = midpoint(line[2], line[3]);
            let left = midpoint(line[3], line[0]);
            vertical_vector[0] += bottom[0] - top[0];
            vertical_vector[1] += bottom[1] - top[1];
            horizontal_vector[0] += right[0] - left[0];
            horizontal_vector[1] += right[1] - left[1];
        }
        let vertical_norm = vector_norm(vertical_vector);
        let horizontal_norm = vector_norm(horizontal_vector);
        if self.vertical {
            self.vector = vertical_vector;
            self.norm = vertical_norm;
            self.font_size = horizontal_norm / self.lines.len() as f32;
            self.angle = vertical_vector[1].atan2(vertical_vector[0]).to_degrees() - 90.0;
        } else {
            self.vector = horizontal_vector;
            self.norm = horizontal_norm;
            self.font_size = vertical_norm / self.lines.len() as f32;
            self.angle = horizontal_vector[1]
                .atan2(horizontal_vector[0])
                .to_degrees();
        }
        if self.angle.abs() < 3.0 {
            self.angle = 0.0;
        }
    }

    fn into_public(self) -> ComicTextBlock {
        ComicTextBlock {
            bbox: self.bbox,
            score: self.score,
            class_id: 2,
            label: "unknown".to_string(),
            line_polygons: self.lines,
            vertical: self.vertical,
            rotation_deg: self.angle,
            detected_font_size: self.font_size,
        }
    }
}

fn group_text_lines(lines: &[ScoredQuad], mask: &GrayImage) -> Vec<ComicTextBlock> {
    let mut horizontal = Vec::new();
    let mut vertical = Vec::new();
    for &line in lines {
        let bbox = quad_bbox(&line.quad);
        if mean_mask(mask, bbox) < MASK_SCORE_THRESHOLD {
            continue;
        }
        let block = WorkingBlock::from_line(line);
        if block.vertical {
            vertical.push(block);
        } else {
            horizontal.push(block);
        }
    }
    horizontal.sort_unstable_by(|a, b| a.center()[1].total_cmp(&b.center()[1]));
    vertical.sort_unstable_by(|a, b| b.center()[0].total_cmp(&a.center()[0]));

    let mut grouped = merge_text_lines(horizontal, 2.0);
    grouped.extend(merge_text_lines(vertical, 1.7));
    let mut grouped = sort_regions(grouped);
    let mut kept: Vec<WorkingBlock> = Vec::with_capacity(grouped.len());
    for block in grouped.drain(..) {
        let area = bbox_area(block.bbox).max(f32::EPSILON);
        let contained = kept
            .iter()
            .any(|existing| bbox_intersection(block.bbox, existing.bbox) / area > 0.9);
        if !contained {
            kept.push(block);
        }
    }
    kept.into_iter().map(WorkingBlock::into_public).collect()
}

fn merge_text_lines(mut blocks: Vec<WorkingBlock>, font_size_tolerance: f32) -> Vec<WorkingBlock> {
    if blocks.len() < 2 {
        return blocks;
    }
    let mut merged = Vec::new();
    for index in 0..blocks.len() {
        if blocks[index].merged {
            continue;
        }
        for other in index + 1..blocks.len() {
            let (left, right) = blocks.split_at_mut(other);
            try_merge_text_line(&mut left[index], &mut right[0], font_size_tolerance);
        }
        blocks[index].recalculate();
        merged.push(blocks[index].clone());
    }
    merged
}

fn try_merge_text_line(
    block: &mut WorkingBlock,
    other: &mut WorkingBlock,
    font_size_tolerance: f32,
) -> bool {
    if other.merged || block.font_size <= 0.0 || other.font_size <= 0.0 {
        return false;
    }
    let first_count = block.lines.len();
    let other_count = other.lines.len();
    let average_font_size = (block.font_size * first_count as f32
        + other.font_size * other_count as f32)
        / (first_count + other_count) as f32;
    let cosine = dot(block.vector, other.vector) / (block.norm * other.norm).max(f32::EPSILON);
    let first_bbox = quad_bbox(block.lines.last().expect("block contains a line"));
    let other_bbox = quad_bbox(&other.lines[0]);
    let distance_x = first_bbox[0].max(other_bbox[0]) - first_bbox[2].min(other_bbox[2]);
    let distance_y = first_bbox[1].max(other_bbox[1]) - first_bbox[3].min(other_bbox[3]);
    let first_width = first_bbox[2] - first_bbox[0];
    let other_width = other_bbox[2] - other_bbox[0];
    let first_height = first_bbox[3] - first_bbox[1];
    let other_height = other_bbox[3] - other_bbox[1];

    if !quads_intersect(block.lines.last().unwrap(), &other.lines[0]) {
        if block.vertical {
            if distance_y > 0.0
                || distance_x > average_font_size * 0.8
                || distance_y.abs() / first_height.min(other_height).max(1.0) < 0.4
            {
                return false;
            }
        } else {
            let threshold = if average_font_size < 24.0 { 0.6 } else { 0.5 };
            if distance_x > 0.0
                || distance_y > average_font_size * threshold
                || distance_x.abs() / first_width.min(other_width).max(1.0) < 0.3
            {
                return false;
            }
        }
        let ratio = block.font_size / other.font_size;
        if ratio > font_size_tolerance
            || ratio.recip() > font_size_tolerance
            || cosine.abs() < 0.866
        {
            return false;
        }
    }

    block.lines.extend(other.lines.iter().copied());
    block.score = (block.score * first_count as f32 + other.score * other_count as f32)
        / (first_count + other_count) as f32;
    block.recalculate();
    other.merged = true;
    true
}

fn sort_regions(regions: Vec<WorkingBlock>) -> Vec<WorkingBlock> {
    let right_to_left = regions.iter().any(|region| region.vertical);
    let mut input = regions;
    input.sort_unstable_by(|a, b| a.center()[1].total_cmp(&b.center()[1]));
    let mut output: Vec<WorkingBlock> = Vec::with_capacity(input.len());
    for region in input {
        let mut insertion = None;
        for (index, existing) in output.iter().enumerate() {
            if region.center()[1] > existing.bbox[3] {
                continue;
            }
            if region.center()[1] < existing.bbox[1] {
                insertion = Some((index + 1).min(output.len()));
                break;
            }
            if (right_to_left && region.center()[0] > existing.center()[0])
                || (!right_to_left && region.center()[0] < existing.center()[0])
            {
                insertion = Some(index);
                break;
            }
        }
        if let Some(index) = insertion {
            output.insert(index, region);
        } else {
            output.push(region);
        }
    }
    output
}

fn sort_line_quad(quad: Quad) -> (Quad, bool) {
    let edge_a = distance(quad[0], quad[1]);
    let edge_b = distance(quad[1], quad[2]);
    let square = (edge_a - edge_b).abs() < 1e-3;
    let long_vector = if edge_a >= edge_b {
        [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]]
    } else {
        [quad[2][0] - quad[1][0], quad[2][1] - quad[1][1]]
    };
    let vertical = !square && long_vector[0].abs() * 1.2 <= long_vector[1].abs();
    let mut points = quad;
    if vertical {
        points.sort_unstable_by(|a, b| a[1].total_cmp(&b[1]));
        let mut top = [points[0], points[1]];
        let mut bottom = [points[2], points[3]];
        top.sort_unstable_by(|a, b| a[0].total_cmp(&b[0]));
        bottom.sort_unstable_by(|a, b| b[0].total_cmp(&a[0]));
        ([top[0], top[1], bottom[0], bottom[1]], true)
    } else {
        points.sort_unstable_by(|a, b| a[0].total_cmp(&b[0]));
        let mut left = [points[0], points[1]];
        let mut right = [points[2], points[3]];
        left.sort_unstable_by(|a, b| a[1].total_cmp(&b[1]));
        right.sort_unstable_by(|a, b| a[1].total_cmp(&b[1]));
        ([left[0], right[0], right[1], left[1]], false)
    }
}

fn quads_intersect(a: &Quad, b: &Quad) -> bool {
    [a, b].into_iter().all(|polygon| {
        (0..4).all(|index| {
            let edge = [
                polygon[(index + 1) % 4][0] - polygon[index][0],
                polygon[(index + 1) % 4][1] - polygon[index][1],
            ];
            let axis = [-edge[1], edge[0]];
            let (a_min, a_max) = project_quad(a, axis);
            let (b_min, b_max) = project_quad(b, axis);
            a_max >= b_min && b_max >= a_min
        })
    })
}

fn project_quad(quad: &Quad, axis: [f32; 2]) -> (f32, f32) {
    quad.iter()
        .map(|point| point[0] * axis[0] + point[1] * axis[1])
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), value| {
            (min.min(value), max.max(value))
        })
}

fn mean_mask(mask: &GrayImage, bbox: [f32; 4]) -> f32 {
    let x1 = bbox[0].floor().max(0.0) as u32;
    let y1 = bbox[1].floor().max(0.0) as u32;
    let x2 = bbox[2].ceil().clamp(0.0, mask.width() as f32) as u32;
    let y2 = bbox[3].ceil().clamp(0.0, mask.height() as f32) as u32;
    if x2 <= x1 || y2 <= y1 {
        return 0.0;
    }
    let mut sum = 0u64;
    for y in y1..y2 {
        for x in x1..x2 {
            sum += mask.get_pixel(x, y)[0] as u64;
        }
    }
    sum as f32 / ((x2 - x1) * (y2 - y1)) as f32 / 255.0
}

fn refine_mask(
    source: &DynamicImage,
    predicted: &GrayImage,
    blocks: &[ComicTextBlock],
) -> GrayImage {
    let source = source.to_rgb8();
    let mut refined = GrayImage::new(predicted.width(), predicted.height());
    for block in blocks {
        let [x1, y1, x2, y2] =
            enlarge_window(block.bbox, predicted.width(), predicted.height(), 2.5);
        if x2 <= x1 || y2 <= y1 {
            continue;
        }
        let image_crop = crop_imm(&source, x1, y1, x2 - x1, y2 - y1).to_image();
        let mask_crop = crop_imm(predicted, x1, y1, x2 - x1, y2 - y1).to_image();
        let candidates = mask_candidates(&image_crop, &mask_crop);
        let merged = merge_mask_candidates(candidates, &mask_crop);
        for (x, y, pixel) in merged.enumerate_pixels() {
            if pixel[0] != 0 {
                refined.put_pixel(x1 + x, y1 + y, Luma([255]));
            }
        }
    }
    refined
}

fn enlarge_window(bbox: [f32; 4], image_width: u32, image_height: u32, ratio: f32) -> [u32; 4] {
    let x1 = bbox[0].floor().clamp(0.0, image_width as f32) as i32;
    let y1 = bbox[1].floor().clamp(0.0, image_height as f32) as i32;
    let x2 = bbox[2].ceil().clamp(0.0, image_width as f32) as i32;
    let y2 = bbox[3].ceil().clamp(0.0, image_height as f32) as i32;
    let width = (x2 - x1).max(0) as f32;
    let height = (y2 - y1).max(0) as f32;
    if width <= 0.0 || height <= 0.0 {
        return [0, 0, 0, 0];
    }
    let b = width + height;
    let c = (1.0 - ratio) * width * height;
    let root = (-b + (b * b - 4.0 * c).sqrt()) * 0.5;
    let requested = (root * 0.5).round().max(0.0) as i32;
    let delta_x = requested.min(x1).min(image_width as i32 - x2).max(0);
    let delta_y = requested.min(y1).min(image_height as i32 - y2).max(0);
    [
        (x1 - delta_x) as u32,
        (y1 - delta_y) as u32,
        (x2 + delta_x) as u32,
        (y2 + delta_y) as u32,
    ]
}

struct MaskCandidate {
    mask: GrayImage,
    xor: u64,
}

fn mask_candidates(image: &RgbImage, predicted: &GrayImage) -> Vec<MaskCandidate> {
    let mut candidates = top_color_masks(image, predicted);
    let mut otsu_candidates = (0..3)
        .map(|channel| {
            let channel = GrayImage::from_fn(image.width(), image.height(), |x, y| {
                Luma([image.get_pixel(x, y)[channel]])
            });
            let level = otsu_level(&channel);
            let thresholded = GrayImage::from_fn(channel.width(), channel.height(), |x, y| {
                Luma([if channel.get_pixel(x, y)[0] > level {
                    255
                } else {
                    0
                }])
            });
            best_polarity(thresholded, predicted)
        })
        .collect::<Vec<_>>();
    otsu_candidates.sort_unstable_by_key(|candidate| candidate.xor);
    if let Some(best) = otsu_candidates.into_iter().next() {
        candidates.push(best);
    }
    candidates
}

fn top_color_masks(image: &RgbImage, predicted: &GrayImage) -> Vec<MaskCandidate> {
    let gray = DynamicImage::ImageRgb8(image.clone()).to_luma8();
    let eroded = erode_gray(predicted, 1, MorphShape::Square);
    let mut histogram = [0u32; 256];
    for (gray_pixel, mask_pixel) in gray.pixels().zip(eroded.pixels()) {
        if mask_pixel[0] > 127 {
            histogram[gray_pixel[0] as usize] += 1;
        }
    }
    let total = histogram.iter().copied().sum::<u32>();
    if total == 0 {
        return Vec::new();
    }
    let mut colors = (0..256usize).collect::<Vec<_>>();
    colors.sort_unstable_by(|a, b| histogram[*b].cmp(&histogram[*a]).then_with(|| a.cmp(b)));
    let tolerance = total as f32 * 0.001;
    let mut selected = Vec::new();
    for color in colors {
        if selected
            .iter()
            .all(|selected: &usize| selected.abs_diff(color) > 10)
        {
            selected.push(color);
        }
        if selected.len() >= 3 || (histogram[color] as f32) < tolerance {
            break;
        }
    }
    selected
        .into_iter()
        .map(|color| {
            let top = (color + 30).min(255) as u8;
            let bottom = top.saturating_sub(60);
            let thresholded = GrayImage::from_fn(gray.width(), gray.height(), |x, y| {
                let value = gray.get_pixel(x, y)[0];
                Luma([if value >= bottom && value <= top {
                    255
                } else {
                    0
                }])
            });
            best_polarity(thresholded, predicted)
        })
        .collect()
}

fn best_polarity(mask: GrayImage, predicted: &GrayImage) -> MaskCandidate {
    let normal_xor = xor_sum(&mask, predicted);
    let inverted = GrayImage::from_fn(mask.width(), mask.height(), |x, y| {
        Luma([255 - mask.get_pixel(x, y)[0]])
    });
    let inverted_xor = xor_sum(&inverted, predicted);
    if inverted_xor < normal_xor {
        MaskCandidate {
            mask: inverted,
            xor: inverted_xor,
        }
    } else {
        MaskCandidate {
            mask,
            xor: normal_xor,
        }
    }
}

fn merge_mask_candidates(mut candidates: Vec<MaskCandidate>, predicted: &GrayImage) -> GrayImage {
    candidates.sort_unstable_by_key(|candidate| candidate.xor);
    let predicted = erode_gray(predicted, 1, MorphShape::Ellipse);
    let predicted = GrayImage::from_fn(predicted.width(), predicted.height(), |x, y| {
        Luma([if predicted.get_pixel(x, y)[0] > 60 {
            255
        } else {
            0
        }])
    });
    let mut merged = GrayImage::new(predicted.width(), predicted.height());
    for candidate in candidates {
        for component in connected_components(&candidate.mask, true, true) {
            let bbox_area =
                (component.max_x - component.min_x + 1) * (component.max_y - component.min_y + 1);
            if bbox_area >= 3 {
                accept_component(&mut merged, &predicted, &component.pixels);
            }
        }
    }
    merged = dilate_binary(&merged, 2, MorphShape::Square);

    let holes = connected_components(&merged, false, true);
    let mut areas = holes
        .iter()
        .map(|component| component.pixels.len())
        .collect::<Vec<_>>();
    areas.sort_unstable();
    let area_threshold = if areas.len() > 1 {
        areas[areas.len() - 2]
    } else {
        areas.last().copied().unwrap_or(0)
    };
    for component in holes {
        if component.pixels.len() < area_threshold {
            accept_component(&mut merged, &predicted, &component.pixels);
        }
    }
    merged
}

struct Component {
    pixels: Vec<(u32, u32)>,
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
}

fn connected_components(
    image: &GrayImage,
    foreground_white: bool,
    diagonal: bool,
) -> Vec<Component> {
    let width = image.width();
    let height = image.height();
    let mut visited = vec![false; width as usize * height as usize];
    let mut components = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let index = y as usize * width as usize + x as usize;
            let foreground = image.get_pixel(x, y)[0] != 0;
            if visited[index] || foreground != foreground_white {
                continue;
            }
            visited[index] = true;
            let mut queue = VecDeque::from([(x, y)]);
            let mut pixels = Vec::new();
            let mut min_x = x;
            let mut min_y = y;
            let mut max_x = x;
            let mut max_y = y;
            while let Some((cx, cy)) = queue.pop_front() {
                pixels.push((cx, cy));
                min_x = min_x.min(cx);
                min_y = min_y.min(cy);
                max_x = max_x.max(cx);
                max_y = max_y.max(cy);
                for (nx, ny) in neighbors(width, height, cx, cy, diagonal) {
                    let next_index = ny as usize * width as usize + nx as usize;
                    let next_foreground = image.get_pixel(nx, ny)[0] != 0;
                    if !visited[next_index] && next_foreground == foreground_white {
                        visited[next_index] = true;
                        queue.push_back((nx, ny));
                    }
                }
            }
            components.push(Component {
                pixels,
                min_x,
                min_y,
                max_x,
                max_y,
            });
        }
    }
    components
}

fn neighbors(
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    diagonal: bool,
) -> impl Iterator<Item = (u32, u32)> {
    let mut output = [(0u32, 0u32); 8];
    let mut count = 0usize;
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if (dx == 0 && dy == 0) || (!diagonal && dx != 0 && dy != 0) {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx >= 0 && ny >= 0 && nx < width as i32 && ny < height as i32 {
                output[count] = (nx as u32, ny as u32);
                count += 1;
            }
        }
    }
    output.into_iter().take(count)
}

fn accept_component(merged: &mut GrayImage, predicted: &GrayImage, pixels: &[(u32, u32)]) {
    let mut before = 0u64;
    let mut after = 0u64;
    for &(x, y) in pixels {
        if merged.get_pixel(x, y)[0] == 0 {
            let predicted = predicted.get_pixel(x, y)[0] as u64;
            before += predicted;
            after += 255 - predicted;
        }
    }
    if after < before {
        for &(x, y) in pixels {
            merged.put_pixel(x, y, Luma([255]));
        }
    }
}

#[derive(Clone, Copy)]
enum MorphShape {
    Square,
    Ellipse,
}

fn erode_gray(image: &GrayImage, radius: u32, shape: MorphShape) -> GrayImage {
    if radius == 0 {
        return image.clone();
    }
    let offsets = kernel_offsets(radius, shape);
    GrayImage::from_fn(image.width(), image.height(), |x, y| {
        let mut value = u8::MAX;
        for &(dx, dy) in &offsets {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx >= 0 && ny >= 0 && nx < image.width() as i32 && ny < image.height() as i32 {
                value = value.min(image.get_pixel(nx as u32, ny as u32)[0]);
            }
        }
        Luma([value])
    })
}

fn dilate_binary(image: &GrayImage, radius: u32, shape: MorphShape) -> GrayImage {
    if radius == 0 {
        return image.clone();
    }
    let offsets = kernel_offsets(radius, shape);
    GrayImage::from_fn(image.width(), image.height(), |x, y| {
        let on = offsets.iter().any(|&(dx, dy)| {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            nx >= 0
                && ny >= 0
                && nx < image.width() as i32
                && ny < image.height() as i32
                && image.get_pixel(nx as u32, ny as u32)[0] != 0
        });
        Luma([if on { 255 } else { 0 }])
    })
}

fn kernel_offsets(radius: u32, shape: MorphShape) -> Vec<(i32, i32)> {
    let radius = radius as i32;
    let mut output = Vec::with_capacity(((radius * 2 + 1) * (radius * 2 + 1)) as usize);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let inside = match shape {
                MorphShape::Square => true,
                MorphShape::Ellipse => dx * dx + dy * dy <= radius * radius + radius / 2,
            };
            if inside {
                output.push((dx, dy));
            }
        }
    }
    output
}

fn xor_sum(a: &GrayImage, b: &GrayImage) -> u64 {
    a.pixels()
        .zip(b.pixels())
        .map(|(a, b)| (a[0] ^ b[0]) as u64)
        .sum()
}

fn midpoint(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn vector_norm(vector: [f32; 2]) -> f32 {
    vector[0].hypot(vector[1])
}

fn quad_bbox(quad: &Quad) -> [f32; 4] {
    quad.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |bbox, point| {
            [
                bbox[0].min(point[0]),
                bbox[1].min(point[1]),
                bbox[2].max(point[0]),
                bbox[3].max(point[1]),
            ]
        },
    )
}

fn lines_bbox(lines: &[Quad]) -> [f32; 4] {
    lines.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |bbox, line| {
            let line = quad_bbox(line);
            [
                bbox[0].min(line[0]),
                bbox[1].min(line[1]),
                bbox[2].max(line[2]),
                bbox[3].max(line[3]),
            ]
        },
    )
}

fn bbox_area(bbox: [f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).max(0.0) * (bbox[3] - bbox[1]).max(0.0)
}

fn bbox_intersection(a: [f32; 4], b: [f32; 4]) -> f32 {
    (a[2].min(b[2]) - a[0].max(b[0])).max(0.0) * (a[3].min(b[3]) - a[1].max(b[1])).max(0.0)
}
