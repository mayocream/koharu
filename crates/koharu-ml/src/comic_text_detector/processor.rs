use anyhow::{Context, Result, bail};
use image::{DynamicImage, GenericImageView, GrayImage, Luma, imageops::FilterType};
use koharu_torch::{Device, Kind, Tensor};
use serde::Serialize;

use super::model::ComicTextDetectorForwardOutput;

const LABELS: [&str; 2] = ["eng", "ja"];
const DBNET_BINARIZE_K: f64 = 50.0;
const LINE_BINARY_THRESHOLD: u8 = 76;
const LINE_SCORE_THRESHOLD: f32 = 0.6;
const MAX_LINE_COMPONENTS: usize = 1000;
const MIN_LINE_AREA: u32 = 4;
const BBOX_DILATION: f32 = 1.0;

pub type Quad = [[f32; 2]; 4];

#[derive(Debug, Clone)]
pub struct ComicTextDetectorConfig {
    pub detect_size: u32,
    pub confidence_threshold: f32,
    pub nms_threshold: f32,
    pub mask_threshold: u8,
}

impl Default for ComicTextDetectorConfig {
    fn default() -> Self {
        Self {
            detect_size: 1024,
            confidence_threshold: 0.4,
            nms_threshold: 0.35,
            mask_threshold: 60,
        }
    }
}

#[derive(Debug)]
pub struct PreprocessedImage {
    pub pixel_values: Tensor,
    pub original_width: u32,
    pub original_height: u32,
    pub resized_width: u32,
    pub resized_height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicTextBlock {
    pub bbox: [f32; 4],
    pub score: f32,
    pub class_id: usize,
    pub label: String,
    pub line_polygons: Vec<Quad>,
}

#[derive(Debug, Clone)]
pub struct ComicTextDetection {
    pub image_width: u32,
    pub image_height: u32,
    pub blocks: Vec<ComicTextBlock>,
    pub line_polygons: Vec<Quad>,
    pub mask: GrayImage,
    pub shrink_map: GrayImage,
    pub threshold_map: GrayImage,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicTextDetectionJson {
    pub image_width: u32,
    pub image_height: u32,
    pub blocks: Vec<ComicTextBlock>,
    pub line_polygons: Vec<Quad>,
}

impl ComicTextDetection {
    pub fn to_json(&self) -> ComicTextDetectionJson {
        ComicTextDetectionJson {
            image_width: self.image_width,
            image_height: self.image_height,
            blocks: self.blocks.clone(),
            line_polygons: self.line_polygons.clone(),
        }
    }
}

pub fn preprocess(
    image: &DynamicImage,
    device: Device,
    config: &ComicTextDetectorConfig,
) -> Result<PreprocessedImage> {
    let (original_width, original_height) = image.dimensions();
    if original_width == 0 || original_height == 0 {
        bail!("empty image");
    }

    let detect_size = config.detect_size.max(64);
    let scale = (detect_size as f32 / original_width as f32)
        .min(detect_size as f32 / original_height as f32);
    let resized_width = ((original_width as f32 * scale).round() as u32).max(1);
    let resized_height = ((original_height as f32 * scale).round() as u32).max(1);
    let resized = image::imageops::resize(
        &image.to_rgb8(),
        resized_width,
        resized_height,
        FilterType::Triangle,
    );

    let side = detect_size as usize;
    let plane = side * side;
    let mut data = vec![0.0_f32; 3 * plane];
    for (x, y, pixel) in resized.enumerate_pixels() {
        let offset = y as usize * side + x as usize;
        data[offset] = pixel[0] as f32 / 255.0;
        data[plane + offset] = pixel[1] as f32 / 255.0;
        data[2 * plane + offset] = pixel[2] as f32 / 255.0;
    }

    let pixel_values = Tensor::from_slice(&data)
        .view([1, 3, detect_size as i64, detect_size as i64])
        .to_device(device);
    Ok(PreprocessedImage {
        pixel_values,
        original_width,
        original_height,
        resized_width,
        resized_height,
    })
}

pub fn postprocess(
    outputs: ComicTextDetectorForwardOutput,
    image: &PreprocessedImage,
    config: &ComicTextDetectorConfig,
) -> Result<ComicTextDetection> {
    let mut blocks = postprocess_yolo(&outputs.predictions, image, config)?;
    let shrink_map = tensor_channel_to_gray_resized(
        outputs
            .line_maps
            .select(1, 0)
            .squeeze_dim(0)
            .narrow(0, 0, image.resized_height as i64)
            .narrow(1, 0, image.resized_width as i64),
        image.original_width,
        image.original_height,
    )?;
    let threshold_map = tensor_channel_to_gray_resized(
        outputs
            .line_maps
            .select(1, 1)
            .squeeze_dim(0)
            .narrow(0, 0, image.resized_height as i64)
            .narrow(1, 0, image.resized_width as i64),
        image.original_width,
        image.original_height,
    )?;
    let mask = fused_mask(outputs.mask, outputs.line_maps, image)?;
    let line_polygons = extract_line_polygons(&shrink_map);
    attach_lines_to_blocks(&mut blocks, &line_polygons);

    Ok(ComicTextDetection {
        image_width: image.original_width,
        image_height: image.original_height,
        blocks,
        line_polygons,
        mask,
        shrink_map,
        threshold_map,
    })
}

#[derive(Clone, Copy, Debug)]
struct DetectionBBox {
    xmin: f32,
    ymin: f32,
    xmax: f32,
    ymax: f32,
    confidence: f32,
    class_id: usize,
}

fn postprocess_yolo(
    predictions: &Tensor,
    image: &PreprocessedImage,
    config: &ComicTextDetectorConfig,
) -> Result<Vec<ComicTextBlock>> {
    let shape = predictions.size();
    if shape.len() != 3 || shape[0] != 1 || shape[2] < 6 {
        bail!("invalid comic text detector prediction shape: {shape:?}");
    }

    let num_predictions = shape[1] as usize;
    let num_outputs = shape[2] as usize;
    let num_classes = num_outputs - 5;
    let values = tensor_to_f32_vec(predictions)?;
    let width_ratio = image.original_width as f32 / image.resized_width as f32;
    let height_ratio = image.original_height as f32 / image.resized_height as f32;
    let mut boxes_by_class = (0..num_classes).map(|_| Vec::new()).collect::<Vec<_>>();

    for index in 0..num_predictions {
        let base = index * num_outputs;
        let prediction = &values[base..base + num_outputs];
        let (class_id, class_score) = prediction[5..]
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, 0.0));
        let confidence = prediction[4] * class_score;
        if confidence < config.confidence_threshold {
            continue;
        }

        let xmin = ((prediction[0] - prediction[2] * 0.5) * width_ratio - BBOX_DILATION)
            .clamp(0.0, image.original_width as f32);
        let ymin = ((prediction[1] - prediction[3] * 0.5) * height_ratio - BBOX_DILATION)
            .clamp(0.0, image.original_height as f32);
        let xmax = ((prediction[0] + prediction[2] * 0.5) * width_ratio + BBOX_DILATION)
            .clamp(0.0, image.original_width as f32);
        let ymax = ((prediction[1] + prediction[3] * 0.5) * height_ratio + BBOX_DILATION)
            .clamp(0.0, image.original_height as f32);
        if xmax <= xmin || ymax <= ymin {
            continue;
        }

        boxes_by_class[class_id].push(DetectionBBox {
            xmin,
            ymin,
            xmax,
            ymax,
            confidence,
            class_id,
        });
    }

    non_maximum_suppression(&mut boxes_by_class, config.nms_threshold);
    let mut blocks = boxes_by_class
        .into_iter()
        .flatten()
        .map(|bbox| ComicTextBlock {
            bbox: [bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax],
            score: bbox.confidence,
            class_id: bbox.class_id,
            label: LABELS
                .get(bbox.class_id)
                .copied()
                .unwrap_or("text")
                .to_string(),
            line_polygons: Vec::new(),
        })
        .collect::<Vec<_>>();
    blocks.sort_unstable_by(|a, b| {
        let acy = (a.bbox[1] + a.bbox[3]) * 0.5;
        let bcy = (b.bbox[1] + b.bbox[3]) * 0.5;
        acy.total_cmp(&bcy)
    });
    Ok(blocks)
}

fn fused_mask(mask: Tensor, line_maps: Tensor, image: &PreprocessedImage) -> Result<GrayImage> {
    let mask = mask.select(1, 0).squeeze_dim(0);
    let shrink = line_maps.select(1, 0).squeeze_dim(0);
    let threshold = line_maps.select(1, 1).squeeze_dim(0);
    let mask_shape = mask.size();
    let shrink_shape = shrink.size();
    let height = mask_shape[0]
        .min(shrink_shape[0])
        .min(image.resized_height as i64);
    let width = mask_shape[1]
        .min(shrink_shape[1])
        .min(image.resized_width as i64);
    let mask = mask.narrow(0, 0, height).narrow(1, 0, width);
    let shrink = shrink.narrow(0, 0, height).narrow(1, 0, width);
    let threshold = threshold.narrow(0, 0, height).narrow(1, 0, width);
    let db_prob = ((shrink - threshold) * DBNET_BINARIZE_K).sigmoid();
    tensor_channel_to_gray_resized(
        mask.maximum(&db_prob),
        image.original_width,
        image.original_height,
    )
}

fn tensor_channel_to_gray_resized(tensor: Tensor, width: u32, height: u32) -> Result<GrayImage> {
    let shape = tensor.size();
    if shape.len() != 2 {
        bail!("expected 2D tensor for gray image, got {shape:?}");
    }
    let tensor_width = shape[1] as u32;
    let tensor_height = shape[0] as u32;
    let values = tensor_to_f32_vec(&tensor)?;
    let pixels = values
        .into_iter()
        .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect::<Vec<_>>();
    let gray = GrayImage::from_raw(tensor_width, tensor_height, pixels)
        .context("failed to create gray image from comic text detector tensor")?;
    if tensor_width == width && tensor_height == height {
        return Ok(gray);
    }
    Ok(image::imageops::resize(
        &gray,
        width,
        height,
        FilterType::Triangle,
    ))
}

fn extract_line_polygons(map: &GrayImage) -> Vec<Quad> {
    let width = map.width();
    let height = map.height();
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let mut visited = vec![false; (width as usize) * (height as usize)];
    let mut lines = Vec::new();
    let mut queue = Vec::new();

    for y in 0..height {
        for x in 0..width {
            let index = pixel_index(width, x, y);
            if visited[index] || map.get_pixel(x, y)[0] < LINE_BINARY_THRESHOLD {
                continue;
            }

            visited[index] = true;
            queue.clear();
            queue.push((x, y));
            let mut cursor = 0usize;
            let mut min_x = x;
            let mut min_y = y;
            let mut max_x = x;
            let mut max_y = y;
            let mut sum = 0u32;
            let mut count = 0u32;

            while cursor < queue.len() {
                let (cx, cy) = queue[cursor];
                cursor += 1;
                let value = map.get_pixel(cx, cy)[0] as u32;
                sum += value;
                count += 1;
                min_x = min_x.min(cx);
                min_y = min_y.min(cy);
                max_x = max_x.max(cx);
                max_y = max_y.max(cy);

                for (nx, ny) in neighbors(width, height, cx, cy) {
                    let next_index = pixel_index(width, nx, ny);
                    if visited[next_index] || map.get_pixel(nx, ny)[0] < LINE_BINARY_THRESHOLD {
                        continue;
                    }
                    visited[next_index] = true;
                    queue.push((nx, ny));
                }
            }

            if count < MIN_LINE_AREA {
                continue;
            }
            let score = sum as f32 / count as f32 / 255.0;
            if score < LINE_SCORE_THRESHOLD {
                continue;
            }
            lines.push([
                [min_x as f32, min_y as f32],
                [(max_x + 1) as f32, min_y as f32],
                [(max_x + 1) as f32, (max_y + 1) as f32],
                [min_x as f32, (max_y + 1) as f32],
            ]);
            if lines.len() >= MAX_LINE_COMPONENTS {
                return lines;
            }
        }
    }

    lines
}

fn attach_lines_to_blocks(blocks: &mut [ComicTextBlock], lines: &[Quad]) {
    for line in lines {
        let bbox = quad_bbox(line);
        let center_x = (bbox[0] + bbox[2]) * 0.5;
        let center_y = (bbox[1] + bbox[3]) * 0.5;
        if let Some(block) = blocks.iter_mut().find(|block| {
            center_x >= block.bbox[0]
                && center_x <= block.bbox[2]
                && center_y >= block.bbox[1]
                && center_y <= block.bbox[3]
        }) {
            block.line_polygons.push(*line);
        }
    }
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

fn non_maximum_suppression(boxes_by_class: &mut [Vec<DetectionBBox>], threshold: f32) {
    for boxes in boxes_by_class {
        boxes.sort_unstable_by(|a, b| b.confidence.total_cmp(&a.confidence));
        let mut keep = Vec::with_capacity(boxes.len());
        for bbox in boxes.drain(..) {
            if keep.iter().all(|kept| bbox_iou(&bbox, kept) <= threshold) {
                keep.push(bbox);
            }
        }
        *boxes = keep;
    }
}

fn bbox_iou(a: &DetectionBBox, b: &DetectionBBox) -> f32 {
    let inter_width = (a.xmax.min(b.xmax) - a.xmin.max(b.xmin)).max(0.0);
    let inter_height = (a.ymax.min(b.ymax) - a.ymin.max(b.ymin)).max(0.0);
    let intersection = inter_width * inter_height;
    if intersection <= 0.0 {
        return 0.0;
    }
    let area_a = (a.xmax - a.xmin).max(0.0) * (a.ymax - a.ymin).max(0.0);
    let area_b = (b.xmax - b.xmin).max(0.0) * (b.ymax - b.ymin).max(0.0);
    intersection / (area_a + area_b - intersection).max(f32::EPSILON)
}

fn tensor_to_f32_vec(tensor: &Tensor) -> Result<Vec<f32>> {
    let tensor = tensor
        .to_device(Device::Cpu)
        .to_kind(Kind::Float)
        .contiguous()
        .view([-1]);
    Ok(Vec::<f32>::try_from(&tensor)?)
}

fn pixel_index(width: u32, x: u32, y: u32) -> usize {
    y as usize * width as usize + x as usize
}

fn neighbors(width: u32, height: u32, x: u32, y: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut values = [(u32::MAX, u32::MAX); 4];
    let mut len = 0usize;
    if x > 0 {
        values[len] = (x - 1, y);
        len += 1;
    }
    if y > 0 {
        values[len] = (x, y - 1);
        len += 1;
    }
    if x + 1 < width {
        values[len] = (x + 1, y);
        len += 1;
    }
    if y + 1 < height {
        values[len] = (x, y + 1);
        len += 1;
    }
    values.into_iter().take(len)
}

pub fn threshold_mask(mask: &GrayImage, threshold: u8) -> GrayImage {
    GrayImage::from_fn(mask.width(), mask.height(), |x, y| {
        if mask.get_pixel(x, y)[0] >= threshold {
            Luma([255])
        } else {
            Luma([0])
        }
    })
}
