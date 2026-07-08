use std::cmp::Ordering;

use anyhow::{Context, Result, bail};
use image::{DynamicImage, GenericImageView, GrayImage, Luma, imageops::FilterType};
use imageproc::{
    contours::{BorderType, find_contours_with_threshold},
    geometry::{approximate_polygon_dp, arc_length, contour_area},
};
use koharu_torch::{Device, IndexOp, Kind, Tensor};
use serde::{Deserialize, Serialize};

use super::model::PPDocLayoutV3ForwardOutput;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PPDocLayoutV3Processor {
    pub size: ProcessorSize,
    pub labels: Vec<String>,
}

impl Default for PPDocLayoutV3Processor {
    fn default() -> Self {
        Self {
            size: ProcessorSize {
                height: 800,
                width: 800,
            },
            labels: Vec::new(),
        }
    }
}

impl PPDocLayoutV3Processor {
    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = labels;
        self
    }

    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn preprocess(&self, image: &DynamicImage, device: Device) -> Tensor {
        let rgb = image.to_rgb8();
        let (width, height) = rgb.dimensions();

        let mut pixels = Vec::with_capacity((height * width * 3) as usize);
        for pixel in rgb.pixels() {
            pixels.push(pixel[0] as f32);
            pixels.push(pixel[1] as f32);
            pixels.push(pixel[2] as f32);
        }

        let pixel_values = Tensor::from_slice(&pixels)
            .view([1, height as i64, width as i64, 3])
            .permute([0, 3, 1, 2])
            .to_device(device);

        pixel_values
            .upsample_bicubic2d([self.size.height, self.size.width], false, None, None)
            .clamp(0.0, 255.0)
            .round()
            / 255.0
    }

    pub fn postprocess(
        &self,
        outputs: &PPDocLayoutV3ForwardOutput,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<PPDocLayoutV3Detections> {
        let (target_width, target_height) = image.dimensions();
        let mut detections =
            self.postprocess_batch(outputs, &[(target_height, target_width)], threshold)?;
        detections.pop().context("missing PP-DocLayout-V3 result")
    }

    /// `target_sizes` follows Transformers and uses `(height, width)`.
    pub fn postprocess_batch(
        &self,
        outputs: &PPDocLayoutV3ForwardOutput,
        target_sizes: &[(u32, u32)],
        threshold: f32,
    ) -> Result<Vec<PPDocLayoutV3Detections>> {
        let logits = &outputs.logits;
        let batch_size = logits.size()[0] as usize;
        if target_sizes.len() != batch_size {
            bail!(
                "target size count {} does not match PP-DocLayout-V3 batch size {}",
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

        let mask_size = outputs.out_masks.size();
        let mask_height = mask_size[2] as usize;
        let mask_width = mask_size[3] as usize;
        let masks = outputs.out_masks.gather(
            1,
            &query_index
                .unsqueeze(-1)
                .unsqueeze(-1)
                .repeat([1, 1, mask_size[2], mask_size[3]]),
            false,
        );
        let masks = masks.sigmoid().gt(threshold as f64);

        let order_seq = get_order_seq(&outputs.order_logits).gather(1, &query_index, false);

        let mut results = Vec::with_capacity(batch_size);
        let mask_area = mask_height * mask_width;
        for batch in 0..batch_size {
            let scores = tensor_to_vec_f32(&scores.i(batch as i64))?;
            let labels = tensor_to_vec_i64(&labels.i(batch as i64))?;
            let boxes = tensor_to_vec_f32(&boxes.i(batch as i64).contiguous().view([-1]))?;
            let orders = tensor_to_vec_i64(&order_seq.i(batch as i64))?;
            let masks = tensor_to_vec_i64(&masks.i(batch as i64).contiguous().view([-1]))?;

            let mut candidates = Vec::new();
            for query in 0..scores.len() {
                let score = scores[query];
                if score < threshold {
                    continue;
                }
                let offset = query * 4;
                let bbox = [
                    boxes[offset],
                    boxes[offset + 1],
                    boxes[offset + 2],
                    boxes[offset + 3],
                ];
                candidates.push(CandidateRegion {
                    query,
                    order: orders[query],
                    label_id: labels[query].max(0) as usize,
                    score,
                    bbox,
                });
            }

            candidates.sort_by_key(|candidate| candidate.order);

            let (target_height, target_width) = target_sizes[batch];
            let mut regions = Vec::with_capacity(candidates.len());
            for (idx, candidate) in candidates.into_iter().enumerate() {
                let label = self
                    .labels
                    .get(candidate.label_id)
                    .cloned()
                    .unwrap_or_else(|| format!("LABEL_{}", candidate.label_id));
                let mask_offset = candidate.query * mask_area;
                let mask = &masks[mask_offset..mask_offset + mask_area];
                regions.push(PPDocLayoutV3Region {
                    order: idx + 1,
                    label_id: candidate.label_id,
                    label,
                    score: candidate.score,
                    bbox: candidate.bbox,
                    polygon_points: polygon_from_mask(
                        candidate.bbox,
                        mask,
                        mask_width,
                        mask_height,
                        target_width,
                        target_height,
                    ),
                });
            }

            results.push(PPDocLayoutV3Detections { regions });
        }

        Ok(results)
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
            height: 800,
            width: 800,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PPDocLayoutV3Detections {
    pub regions: Vec<PPDocLayoutV3Region>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PPDocLayoutV3Region {
    pub order: usize,
    pub label_id: usize,
    pub label: String,
    pub score: f32,
    pub bbox: [f32; 4],
    pub polygon_points: Vec<[f32; 2]>,
}

struct CandidateRegion {
    query: usize,
    order: i64,
    label_id: usize,
    score: f32,
    bbox: [f32; 4],
}

fn center_to_corners(boxes: &Tensor) -> Tensor {
    let centers = boxes.slice(-1, 0, 2, 1);
    let dims = boxes.slice(-1, 2, 4, 1);
    let top_left = &centers - &dims * 0.5;
    let bottom_right = centers + dims * 0.5;
    Tensor::cat(&[top_left, bottom_right], -1)
}

fn get_order_seq(order_logits: &Tensor) -> Tensor {
    let order_scores = order_logits.sigmoid();
    let size = order_scores.size();
    let batch_size = size[0];
    let sequence_length = size[1];

    let upper_votes = order_scores
        .triu(1)
        .sum_dim_intlist(&[1i64][..], false, None::<Kind>);
    let transposed_scores = order_scores.transpose(1, 2);
    let lower_votes = (transposed_scores.ones_like() - transposed_scores)
        .tril(-1)
        .sum_dim_intlist(&[1i64][..], false, None::<Kind>);
    let order_votes = upper_votes + lower_votes;
    let order_pointers = order_votes.argsort(1, false);
    let ranks = Tensor::arange(sequence_length, (Kind::Int64, order_logits.device()))
        .expand([batch_size, sequence_length], true);
    let mut order_seq = order_pointers.empty_like();
    order_seq.scatter_(1, &order_pointers, &ranks)
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

fn polygon_from_mask(
    bbox: [f32; 4],
    mask: &[i64],
    mask_width: usize,
    mask_height: usize,
    target_width: u32,
    target_height: u32,
) -> Vec<[f32; 2]> {
    let x_min = bbox[0] as i32;
    let y_min = bbox[1] as i32;
    let x_max = bbox[2] as i32;
    let y_max = bbox[3] as i32;
    let rect = rect_polygon_i32(x_min, y_min, x_max, y_max);
    let box_width = x_max - x_min;
    let box_height = y_max - y_min;
    if box_width <= 0 || box_height <= 0 {
        return rect;
    }

    let scale_width = mask_width as f32 / target_width as f32;
    let scale_height = mask_height as f32 / target_height as f32;
    let x_start = scaled_bound(x_min, scale_width, mask_width);
    let x_end = scaled_bound(x_max, scale_width, mask_width);
    let y_start = scaled_bound(y_min, scale_height, mask_height);
    let y_end = scaled_bound(y_max, scale_height, mask_height);
    if x_start >= x_end || y_start >= y_end {
        return rect;
    }

    let crop_width = x_end - x_start;
    let crop_height = y_end - y_start;
    let mut crop = GrayImage::new(crop_width as u32, crop_height as u32);
    for y in 0..crop_height {
        for x in 0..crop_width {
            let value = mask[(y_start + y) * mask_width + x_start + x];
            crop.put_pixel(x as u32, y as u32, Luma([if value != 0 { 255 } else { 0 }]));
        }
    }

    let resized = image::imageops::resize(
        &crop,
        box_width as u32,
        box_height as u32,
        FilterType::Nearest,
    );
    let Some(mut polygon) = mask_to_polygon(&resized) else {
        return rect;
    };
    if polygon.len() < 4 {
        return rect;
    }

    for point in &mut polygon {
        point[0] += x_min as f32;
        point[1] += y_min as f32;
    }
    polygon
}

fn mask_to_polygon(mask: &GrayImage) -> Option<Vec<[f32; 2]>> {
    let contours = find_contours_with_threshold::<i32>(mask, 0);
    let contour = contours
        .iter()
        .filter(|contour| contour.border_type == BorderType::Outer)
        .max_by(|left, right| {
            contour_area(&left.points)
                .partial_cmp(&contour_area(&right.points))
                .unwrap_or(Ordering::Equal)
        })?;

    if contour.points.len() < 3 {
        return None;
    }

    let epsilon = 0.004 * arc_length(&contour.points, true);
    let approximated = approximate_polygon_dp(&contour.points, epsilon.max(f64::EPSILON), true);
    if approximated.is_empty() {
        return None;
    }

    let points = approximated
        .into_iter()
        .map(|point| [point.x as f32, point.y as f32])
        .collect::<Vec<_>>();
    Some(extract_custom_vertices(&points))
}

fn extract_custom_vertices(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut vertices = Vec::new();
    for idx in 0..points.len() {
        let previous = points[(idx + points.len() - 1) % points.len()];
        let current = points[idx];
        let next = points[(idx + 1) % points.len()];
        let vector_1 = [previous[0] - current[0], previous[1] - current[1]];
        let vector_2 = [next[0] - current[0], next[1] - current[1]];
        let cross_product = vector_1[1] * vector_2[0] - vector_1[0] * vector_2[1];
        if cross_product >= 0.0 {
            continue;
        }

        let norm_1 = vector_norm(vector_1);
        let norm_2 = vector_norm(vector_2);
        if norm_1 == 0.0 || norm_2 == 0.0 {
            vertices.push(current);
            continue;
        }

        let angle_cos = ((vector_1[0] * vector_2[0] + vector_1[1] * vector_2[1])
            / (norm_1 * norm_2))
            .clamp(-1.0, 1.0);
        let angle = angle_cos.acos().to_degrees();
        if (angle - 45.0).abs() < 1.0 {
            let dir = [
                vector_1[0] / norm_1 + vector_2[0] / norm_2,
                vector_1[1] / norm_1 + vector_2[1] / norm_2,
            ];
            let dir_norm = vector_norm(dir);
            if dir_norm == 0.0 {
                vertices.push(current);
                continue;
            }
            let step = (norm_1 + norm_2) / 2.0;
            vertices.push([
                current[0] + dir[0] / dir_norm * step,
                current[1] + dir[1] / dir_norm * step,
            ]);
        } else {
            vertices.push(current);
        }
    }

    if vertices.is_empty() {
        points.to_vec()
    } else {
        vertices
    }
}

fn vector_norm(vector: [f32; 2]) -> f32 {
    (vector[0] * vector[0] + vector[1] * vector[1]).sqrt()
}

fn scaled_bound(value: i32, scale: f32, limit: usize) -> usize {
    python_round(value as f32 * scale).clamp(0, limit as i32) as usize
}

fn python_round(value: f32) -> i32 {
    let value = value as f64;
    let floor = value.floor();
    let fraction = value - floor;
    let rounded = if (fraction - 0.5).abs() < f64::EPSILON {
        if floor as i64 % 2 == 0 {
            floor
        } else {
            floor + 1.0
        }
    } else {
        value.round()
    };
    rounded as i32
}

fn rect_polygon_i32(x_min: i32, y_min: i32, x_max: i32, y_max: i32) -> Vec<[f32; 2]> {
    vec![
        [x_min as f32, y_min as f32],
        [x_max as f32, y_min as f32],
        [x_max as f32, y_max as f32],
        [x_min as f32, y_max as f32],
    ]
}
