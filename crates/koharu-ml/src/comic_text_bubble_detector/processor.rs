use anyhow::{Context, Result, bail};
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage, imageops::FilterType};
use imageproc::{drawing::draw_hollow_rect_mut, rect::Rect};
use koharu_torch::{Device, IndexOp, Kind, Tensor};
use serde::{Deserialize, Serialize};

use super::model::Output;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ComicTextBubbleProcessor {
    pub size: ProcessorSize,
    pub labels: Vec<String>,
    pub slice_ratio_threshold: f32,
    pub target_slice_ratio: f32,
    pub overlap_height_ratio: f32,
    pub min_slice_height_ratio: f32,
    pub merge_iou_threshold: f32,
    pub duplicate_iou_threshold: f32,
    pub merge_y_distance_threshold: f32,
    pub containment_threshold: f32,
}

impl Default for ComicTextBubbleProcessor {
    fn default() -> Self {
        Self {
            size: ProcessorSize {
                height: 640,
                width: 640,
            },
            labels: Vec::new(),
            slice_ratio_threshold: 3.5,
            target_slice_ratio: 3.0,
            overlap_height_ratio: 0.2,
            min_slice_height_ratio: 0.7,
            merge_iou_threshold: 0.2,
            duplicate_iou_threshold: 0.5,
            merge_y_distance_threshold: 0.1,
            containment_threshold: 0.85,
        }
    }
}

impl ComicTextBubbleProcessor {
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = labels;
        self
    }

    pub fn preprocess(&self, image: &DynamicImage, device: Device) -> Tensor {
        let resized = image.resize_exact(
            self.size.width as u32,
            self.size.height as u32,
            FilterType::Triangle,
        );
        let rgb = resized.to_rgb8();
        let (width, height) = rgb.dimensions();

        let mut pixels = Vec::with_capacity((height * width * 3) as usize);
        for pixel in rgb.pixels() {
            pixels.push(pixel[0] as f32 / 255.0);
            pixels.push(pixel[1] as f32 / 255.0);
            pixels.push(pixel[2] as f32 / 255.0);
        }

        Tensor::from_slice(&pixels)
            .view([1, height as i64, width as i64, 3])
            .permute([0, 3, 1, 2])
            .to_device(device)
    }

    pub fn postprocess(
        &self,
        outputs: &Output,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<ComicTextBubbleDetection> {
        let (target_width, target_height) = image.dimensions();
        let mut detections =
            self.postprocess_batch(outputs, &[(target_height, target_width)], threshold)?;
        detections
            .pop()
            .context("missing comic text/bubble detector result")
    }

    /// `target_sizes` follows Transformers and uses `(height, width)`.
    pub fn postprocess_batch(
        &self,
        outputs: &Output,
        target_sizes: &[(u32, u32)],
        threshold: f32,
    ) -> Result<Vec<ComicTextBubbleDetection>> {
        let logits = &outputs.logits;
        let batch_size = logits.size()[0] as usize;
        if target_sizes.len() != batch_size {
            bail!(
                "target size count {} does not match detector batch size {}",
                target_sizes.len(),
                batch_size
            );
        }

        let pred_boxes = center_to_corners(&outputs.pred_boxes);
        let num_queries = logits.size()[1];
        let num_classes = logits.size()[2];

        let mut scale_values = Vec::with_capacity(batch_size * 4);
        for &(target_height, target_width) in target_sizes {
            scale_values.extend_from_slice(&[
                target_width as f32,
                target_height as f32,
                target_width as f32,
                target_height as f32,
            ]);
        }
        let scale = Tensor::from_slice(&scale_values)
            .view([batch_size as i64, 1, 4])
            .to_device(pred_boxes.device());
        let pred_boxes = pred_boxes * scale;

        let scores_all = logits.sigmoid();
        let (scores, flat_index) = scores_all.flatten(1, -1).topk(num_queries, -1, true, true);
        let labels = flat_index.remainder(num_classes);
        let query_index = flat_index.floor_divide_scalar(num_classes);
        let boxes = pred_boxes.gather(
            1,
            &query_index
                .unsqueeze(-1)
                .repeat([1, 1, pred_boxes.size()[2]]),
            false,
        );

        let mut results = Vec::with_capacity(batch_size);
        for batch in 0..batch_size {
            let scores = tensor_to_vec_f32(&scores.i(batch as i64))?;
            let labels = tensor_to_vec_i64(&labels.i(batch as i64))?;
            let boxes = tensor_to_vec_f32(&boxes.i(batch as i64).contiguous().view([-1]))?;

            let mut regions = Vec::new();
            let (target_height, target_width) = target_sizes[batch];
            for query in 0..scores.len() {
                let score = scores[query];
                if score <= threshold {
                    continue;
                }
                let offset = query * 4;
                let label_id = labels[query].max(0) as usize;
                if label_id > 2 {
                    continue;
                }
                let bbox = [
                    boxes[offset] as i32 as f32,
                    boxes[offset + 1] as i32 as f32,
                    boxes[offset + 2] as i32 as f32,
                    boxes[offset + 3] as i32 as f32,
                ];
                let label = self
                    .labels
                    .get(label_id)
                    .cloned()
                    .unwrap_or_else(|| format!("LABEL_{label_id}"));
                regions.push(ComicTextBubbleRegion {
                    label_id,
                    label,
                    score,
                    bbox,
                });
            }

            let blocks = build_text_blocks(&regions, target_width, target_height);
            results.push(ComicTextBubbleDetection { regions, blocks });
        }

        Ok(results)
    }

    pub fn inference_slices<F>(
        &self,
        image: &DynamicImage,
        mut detect_one: F,
    ) -> Result<ComicTextBubbleDetection>
    where
        F: FnMut(&DynamicImage) -> Result<ComicTextBubbleDetection>,
    {
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 || height as f32 / width as f32 <= self.slice_ratio_threshold {
            return detect_one(image);
        }

        let slice_height = ((width as f32 * self.target_slice_ratio) as u32).max(1);
        let effective_slice_height =
            (slice_height as f32 * (1.0 - self.overlap_height_ratio)) as u32;
        let effective_slice_height = effective_slice_height.max(1);
        let num_slices = height.div_ceil(effective_slice_height);

        let mut regions = Vec::new();
        for slice in 0..num_slices {
            let start_y = slice * effective_slice_height;
            let end_y = if slice + 1 == num_slices {
                height
            } else {
                (start_y + slice_height).min(height)
            };
            let slice_image = image.crop_imm(0, start_y, width, end_y - start_y);
            let mut detection = detect_one(&slice_image)?;
            for region in &mut detection.regions {
                region.bbox[1] += start_y as f32;
                region.bbox[3] += start_y as f32;
            }
            regions.extend(detection.regions);
        }

        let mut bubbles = regions
            .iter()
            .filter(|region| region.label_id == 0)
            .cloned()
            .collect::<Vec<_>>();
        let mut texts = regions
            .iter()
            .filter(|region| region.label_id == 1 || region.label_id == 2)
            .cloned()
            .collect::<Vec<_>>();
        bubbles = self.merge_slice_regions(bubbles, height);
        texts = self.merge_slice_regions(texts, height);

        let mut merged = Vec::with_capacity(bubbles.len() + texts.len());
        merged.extend(bubbles);
        merged.extend(texts);

        let blocks = build_text_blocks(&merged, width, height);
        Ok(ComicTextBubbleDetection {
            regions: merged,
            blocks,
        })
    }

    fn merge_slice_regions(
        &self,
        mut regions: Vec<ComicTextBubbleRegion>,
        image_height: u32,
    ) -> Vec<ComicTextBubbleRegion> {
        let mut index = 0;
        while index + 1 < regions.len() {
            let mut next = index + 1;
            while next < regions.len() {
                let a = regions[index].bbox;
                let b = regions[next].bbox;
                let iou = calculate_iou(a, b);
                let containment = containment_ratio(a, b);
                let area_a = box_area(a);
                let area_b = box_area(b);

                if intersection_area(a, b) > 0.0 && containment >= self.containment_threshold {
                    if area_b >= area_a {
                        regions[index] = regions[next].clone();
                    }
                    regions.remove(next);
                    continue;
                }

                if iou >= self.duplicate_iou_threshold {
                    if area_b > area_a {
                        regions[index] = regions[next].clone();
                    }
                    regions.remove(next);
                    continue;
                }

                let y_dist = (a[1] - b[3]).abs().min((a[3] - b[1]).abs());
                let x_overlap = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0)
                    / box_width(a).min(box_width(b)).max(1.0);
                let area_ratio = area_a.min(area_b) / area_a.max(area_b).max(1.0);
                let local_y_threshold = (self.merge_y_distance_threshold * image_height as f32)
                    .min(box_height(a).max(box_height(b)) * 0.1);
                let same_object = y_dist < local_y_threshold
                    && x_overlap > self.merge_iou_threshold
                    && area_ratio > 0.3
                    && (a[0] - b[0]).abs() < 0.5 * box_width(a).max(box_width(b))
                    && (a[2] - b[2]).abs() < 0.5 * box_width(a).max(box_width(b));

                if same_object {
                    let merged = merge_region(&regions[index], &regions[next]);
                    if box_area(merged.bbox) > 3.0 * area_a.max(area_b) {
                        next += 1;
                        continue;
                    }
                    regions[index] = merged;
                    regions.remove(next);
                } else {
                    next += 1;
                }
            }
            index += 1;
        }
        regions
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProcessorSize {
    pub height: i64,
    pub width: i64,
}

impl Default for ProcessorSize {
    fn default() -> Self {
        Self {
            height: 640,
            width: 640,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicTextBubbleDetection {
    pub regions: Vec<ComicTextBubbleRegion>,
    pub blocks: Vec<ComicTextBubbleBlock>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicTextBubbleRegion {
    pub label_id: usize,
    pub label: String,
    pub score: f32,
    pub bbox: [f32; 4],
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicTextBubbleBlock {
    pub bbox: [f32; 4],
    pub score: f32,
    pub text_class: String,
    pub bubble_bbox: Option<[f32; 4]>,
}

impl ComicTextBubbleDetection {
    pub fn annotated_image(&self, image: &DynamicImage) -> RgbaImage {
        let mut annotated = image.to_rgba8();
        for region in &self.regions {
            let color = match region.label_id {
                0 => Rgba([40, 160, 255, 255]),
                1 => Rgba([40, 220, 90, 255]),
                2 => Rgba([255, 80, 70, 255]),
                _ => Rgba([255, 220, 40, 255]),
            };
            draw_box(&mut annotated, region.bbox, color);
        }
        annotated
    }
}

fn center_to_corners(boxes: &Tensor) -> Tensor {
    let centers = boxes.slice(-1, 0, 2, 1);
    let dims = boxes.slice(-1, 2, 4, 1);
    let top_left = &centers - &dims * 0.5;
    let bottom_right = centers + dims * 0.5;
    Tensor::cat(&[top_left, bottom_right], -1)
}

fn tensor_to_vec_f32(tensor: &Tensor) -> Result<Vec<f32>> {
    let tensor = tensor
        .to_device(Device::Cpu)
        .to_kind(Kind::Float)
        .contiguous();
    let mut values = vec![0f32; tensor.numel()];
    let len = values.len();
    tensor.f_copy_data(&mut values, len)?;
    Ok(values)
}

fn tensor_to_vec_i64(tensor: &Tensor) -> Result<Vec<i64>> {
    let tensor = tensor
        .to_device(Device::Cpu)
        .to_kind(Kind::Int64)
        .contiguous();
    let mut values = vec![0i64; tensor.numel()];
    let len = values.len();
    tensor.f_copy_data(&mut values, len)?;
    Ok(values)
}

fn build_text_blocks(
    regions: &[ComicTextBubbleRegion],
    width: u32,
    height: u32,
) -> Vec<ComicTextBubbleBlock> {
    let bubbles = regions
        .iter()
        .filter(|region| region.label_id == 0)
        .filter_map(|region| {
            filter_box(region.bbox, width, height).map(|bbox| (bbox, region.score))
        })
        .collect::<Vec<_>>();
    let texts = regions
        .iter()
        .filter(|region| region.label_id == 1 || region.label_id == 2)
        .filter_map(|region| {
            filter_box(region.bbox, width, height).map(|bbox| (bbox, region.score))
        })
        .collect::<Vec<_>>();
    let texts = merge_text_boxes(&texts);

    texts
        .into_iter()
        .map(|(bbox, score)| {
            let bubble_bbox = bubbles.iter().find_map(|(bubble_bbox, _)| {
                (does_rectangle_fit(*bubble_bbox, bbox) || calculate_iou(*bubble_bbox, bbox) >= 0.2)
                    .then_some(*bubble_bbox)
            });
            let text_class = if bubble_bbox.is_some() {
                "text_bubble"
            } else {
                "text_free"
            };
            ComicTextBubbleBlock {
                bbox,
                score,
                text_class: text_class.to_owned(),
                bubble_bbox,
            }
        })
        .collect()
}

fn filter_box(mut bbox: [f32; 4], width: u32, height: u32) -> Option<[f32; 4]> {
    bbox[0] = bbox[0].clamp(0.0, width as f32);
    bbox[2] = bbox[2].clamp(0.0, width as f32);
    bbox[1] = bbox[1].clamp(0.0, height as f32);
    bbox[3] = bbox[3].clamp(0.0, height as f32);
    (box_width(bbox) > 5.0 && box_height(bbox) > 5.0).then_some(bbox)
}

fn merge_text_boxes(regions: &[([f32; 4], f32)]) -> Vec<([f32; 4], f32)> {
    let mut accepted = Vec::<([f32; 4], f32)>::new();
    for (index, &(bbox, score)) in regions.iter().enumerate() {
        let mut merged = bbox;
        let mut merged_score = score;
        for (other_index, &(other, other_score)) in regions.iter().enumerate() {
            if index == other_index {
                continue;
            }
            if is_mostly_contained(merged, other, 0.3) || is_mostly_contained(other, merged, 0.3) {
                merged = merge_boxes(merged, other);
                merged_score = merged_score.max(other_score);
            }
        }

        if accepted
            .iter()
            .any(|(bbox, _)| *bbox == merged || calculate_iou(*bbox, merged) >= 0.5)
        {
            continue;
        }
        accepted.retain(|(bbox, _)| *bbox != merged && calculate_iou(*bbox, merged) < 0.5);
        accepted.push((merged, merged_score));
    }
    accepted
}

fn is_mostly_contained(outer: [f32; 4], inner: [f32; 4], threshold: f32) -> bool {
    let outer_area = box_area(outer);
    let inner_area = box_area(inner);
    if outer_area < inner_area || inner_area == 0.0 {
        return false;
    }
    intersection_area(outer, inner) / inner_area >= threshold
}

fn merge_boxes(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn merge_region(a: &ComicTextBubbleRegion, b: &ComicTextBubbleRegion) -> ComicTextBubbleRegion {
    let keep = if a.score >= b.score { a } else { b };
    ComicTextBubbleRegion {
        label_id: keep.label_id,
        label: keep.label.clone(),
        score: keep.score,
        bbox: merge_boxes(a.bbox, b.bbox),
    }
}

fn box_width(bbox: [f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).max(0.0)
}

fn box_height(bbox: [f32; 4]) -> f32 {
    (bbox[3] - bbox[1]).max(0.0)
}

fn box_area(bbox: [f32; 4]) -> f32 {
    box_width(bbox) * box_height(bbox)
}

fn intersection_area(a: [f32; 4], b: [f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = a[2].min(b[2]);
    let y2 = a[3].min(b[3]);
    (x2 - x1).max(0.0) * (y2 - y1).max(0.0)
}

fn calculate_iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let intersection = intersection_area(a, b);
    let union = box_area(a) + box_area(b) - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn containment_ratio(a: [f32; 4], b: [f32; 4]) -> f32 {
    let intersection = intersection_area(a, b);
    let smaller = box_area(a).min(box_area(b));
    if smaller <= 0.0 {
        0.0
    } else {
        intersection / smaller
    }
}

fn does_rectangle_fit(outer: [f32; 4], inner: [f32; 4]) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

fn draw_box(image: &mut RgbaImage, bbox: [f32; 4], color: Rgba<u8>) {
    let x1 = bbox[0].max(0.0).min(image.width() as f32);
    let y1 = bbox[1].max(0.0).min(image.height() as f32);
    let x2 = bbox[2].max(0.0).min(image.width() as f32);
    let y2 = bbox[3].max(0.0).min(image.height() as f32);
    if x2 <= x1 || y2 <= y1 {
        return;
    }
    draw_hollow_rect_mut(
        image,
        Rect::at(x1 as i32, y1 as i32).of_size((x2 - x1) as u32, (y2 - y1) as u32),
        color,
    );
}
