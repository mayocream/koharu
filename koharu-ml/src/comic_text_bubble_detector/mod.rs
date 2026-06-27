mod model;

use std::{path::Path, time::Instant};

use anyhow::{Context, Result, bail};
use burn::{
    store::{ModuleSnapshot, PyTorchToBurnAdapter, SafetensorsStore},
    tensor::{DType, Device, DeviceKind, FloatDType, Tensor, TensorData},
};
use image::{DynamicImage, GenericImageView, imageops::FilterType};
use koharu_runtime::RuntimeManager;
use serde::Serialize;
use tracing::instrument;

use crate::types::TextRegion;

use self::model::{
    IMAGE_SIZE, NUM_LABELS, RTDetrV2ForObjectDetection, RTDetrV2Outputs, cast_module_float,
    tensor_to_f32_vec,
};

const HF_REPO: &str = "ogkalu/comic-text-and-bubble-detector";
const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.3;
const DETECTOR_NAME: &str = "comic-text-bubble-detector";
const RESCALE_FACTOR: f32 = 1.0 / 255.0;
const ID2LABEL: [&str; NUM_LABELS] = ["bubble", "text_bubble", "text_free"];

koharu_runtime::declare_hf_model_package!(
    id: "model:comic-text-bubble-detector:weights",
    repo: "ogkalu/comic-text-and-bubble-detector",
    file: "model.safetensors",
    bootstrap: false,
    order: 115,
);

#[derive(Debug)]
pub struct ComicTextBubbleDetector {
    model: RTDetrV2ForObjectDetection,
    device: Device,
    dtype: DType,
    slicer: ImageSlicer,
}

impl ComicTextBubbleDetector {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let (device, dtype, module_dtype) = make_device(cpu);
        let downloads = runtime.downloads();
        let weights_path = downloads
            .huggingface_model(HF_REPO, "model.safetensors")
            .await?;

        let model = load_model(&weights_path, &device, module_dtype)?;

        Ok(Self {
            model,
            device,
            dtype,
            slicer: ImageSlicer::default(),
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage) -> Result<ComicTextBubbleDetection> {
        self.inference_with_threshold(image, DEFAULT_CONFIDENCE_THRESHOLD)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_threshold(
        &self,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<ComicTextBubbleDetection> {
        let started = Instant::now();
        let detections = self.slicer.process_slices_for_detection(image, |slice| {
            self.detect_single_image(slice, threshold)
        })?;
        let detections = merge_slice_regions(
            filter_and_fix_regions(detections, image.dimensions()),
            image.height(),
        );
        let text_blocks = detections_to_text_blocks(image.dimensions(), &detections);

        tracing::info!(
            width = image.width(),
            height = image.height(),
            detections = detections.len(),
            text_blocks = text_blocks.len(),
            total_ms = started.elapsed().as_millis(),
            "comic text bubble detector timings"
        );

        Ok(ComicTextBubbleDetection {
            image_width: image.width(),
            image_height: image.height(),
            detections,
            text_blocks,
        })
    }

    #[instrument(level = "debug", skip_all)]
    fn detect_single_image(
        &self,
        image: &DynamicImage,
        threshold: f32,
    ) -> Result<Vec<ComicTextBubbleRegion>> {
        let pixel_values = preprocess_image(image, &self.device, self.dtype)?;
        let outputs = self.model.forward(pixel_values)?;
        post_process_object_detection(outputs, image.dimensions(), threshold)
    }
}

fn load_model(
    path: &Path,
    device: &Device,
    module_dtype: FloatDType,
) -> Result<RTDetrV2ForObjectDetection> {
    let mut model = RTDetrV2ForObjectDetection::new(device)?;
    let mut store = SafetensorsStore::from_file(path)
        .with_from_adapter(PyTorchToBurnAdapter)
        .with_key_remapping(
            r"^model\.encoder_input_proj\.(\d+)\.0\.",
            "model.encoder_input_proj.$1.conv.",
        )
        .with_key_remapping(
            r"^model\.encoder_input_proj\.(\d+)\.1\.",
            "model.encoder_input_proj.$1.norm.",
        )
        .with_key_remapping(
            r"^model\.decoder_input_proj\.(\d+)\.0\.",
            "model.decoder_input_proj.$1.conv.",
        )
        .with_key_remapping(
            r"^model\.decoder_input_proj\.(\d+)\.1\.",
            "model.decoder_input_proj.$1.norm.",
        )
        .with_key_remapping(r"^model\.enc_output\.0\.", "model.enc_output.linear.")
        .with_key_remapping(r"^model\.enc_output\.1\.", "model.enc_output.norm.")
        .with_key_remapping(
            r"^model\.encoder\.encoder\.(\d+)\.layers\.(\d+)\.fc([12])\.",
            "model.encoder.encoder.$1.layers.$2.feed_forward.fc$3.",
        )
        .with_key_remapping(
            r"^model\.decoder\.layers\.(\d+)\.fc([12])\.",
            "model.decoder.layers.$1.feed_forward.fc$2.",
        )
        .with_key_remapping(
            r"^model\.backbone\.model\.encoder\.stages\.(\d+)\.layers\.(\d+)\.shortcut\.1\.",
            "model.backbone.model.encoder.stages.$1.layers.$2.shortcut.",
        )
        .skip_enum_variants(true)
        .allow_partial(false);
    let result = model
        .load_from(&mut store)
        .context("failed to mmap/load comic text bubble detector safetensors through Burn store")?;
    if !result.errors.is_empty() {
        bail!(
            "failed to load comic text bubble detector tensors: {}",
            result
        );
    }
    if !result.missing.is_empty() {
        bail!(
            "comic text bubble detector checkpoint is missing tensors: {}",
            result
        );
    }
    Ok(cast_module_float(model, module_dtype))
}

fn make_device(cpu: bool) -> (Device, DType, FloatDType) {
    #[cfg(feature = "cuda")]
    {
        if !cpu {
            let mut device = Device::cuda(0);
            if let Err(error) = device.configure(FloatDType::BF16) {
                tracing::warn!(%error, "failed to configure Burn CUDA default dtype to BF16");
            }
            return (device, DType::BF16, FloatDType::BF16);
        }
    }

    let mut device = Device::wgpu(if cpu {
        DeviceKind::Cpu
    } else {
        DeviceKind::DefaultDevice
    });
    if let Err(error) = device.configure(FloatDType::F32) {
        tracing::warn!(%error, "failed to configure Burn WGPU default dtype to F32");
    }
    (device, DType::F32, FloatDType::F32)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicTextBubbleDetection {
    pub image_width: u32,
    pub image_height: u32,
    pub detections: Vec<ComicTextBubbleRegion>,
    pub text_blocks: Vec<TextRegion>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicTextBubbleRegion {
    pub label_id: usize,
    pub label: String,
    pub score: f32,
    pub bbox: [f32; 4],
}

impl ComicTextBubbleRegion {
    pub fn is_bubble(&self) -> bool {
        self.label_id == 0
    }

    pub fn is_text(&self) -> bool {
        matches!(self.label_id, 1 | 2)
    }
}

fn preprocess_image(image: &DynamicImage, device: &Device, dtype: DType) -> Result<Tensor<4>> {
    let target_h = IMAGE_SIZE;
    let target_w = IMAGE_SIZE;
    let resized = image.resize_exact(target_w as u32, target_h as u32, FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let plane = target_h * target_w;
    let mut data = vec![0.0_f32; 3 * plane];
    for (x, y, pixel) in rgb.enumerate_pixels() {
        let index = y as usize * target_w + x as usize;
        data[index] = pixel[0] as f32 * RESCALE_FACTOR;
        data[plane + index] = pixel[1] as f32 * RESCALE_FACTOR;
        data[2 * plane + index] = pixel[2] as f32 * RESCALE_FACTOR;
    }

    let mut tensor_data = TensorData::new(data, [1, 3, target_h, target_w]);
    device.staging(std::iter::once(&mut tensor_data));
    Ok(Tensor::from_data(tensor_data, (device, dtype)))
}

fn post_process_object_detection(
    outputs: RTDetrV2Outputs,
    target_size: (u32, u32),
    threshold: f32,
) -> Result<Vec<ComicTextBubbleRegion>> {
    let [batch_size, num_queries, num_classes] = outputs.logits.dims();
    if batch_size != 1 {
        bail!("only single-image inference is supported, got batch_size={batch_size}");
    }
    if num_classes != NUM_LABELS {
        bail!(
            "model output label count mismatch: expected {}, got {}",
            NUM_LABELS,
            num_classes
        );
    }

    let logits = tensor_to_f32_vec(outputs.logits)?;
    let pred_boxes = tensor_to_f32_vec(outputs.pred_boxes)?;
    let mut scored = Vec::with_capacity(num_queries * num_classes);
    for query_index in 0..num_queries {
        for class_id in 0..num_classes {
            let index = query_index * num_classes + class_id;
            scored.push((sigmoid_scalar(logits[index]), query_index, class_id));
        }
    }
    scored.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(num_queries);

    let (image_width, image_height) = target_size;
    let image_width = image_width as f32;
    let image_height = image_height as f32;
    let mut detections = Vec::new();
    for (score, query_index, class_id) in scored {
        if score < threshold {
            continue;
        }
        let box_offset = query_index * 4;
        let bbox = scale_box_to_image(
            [
                pred_boxes[box_offset],
                pred_boxes[box_offset + 1],
                pred_boxes[box_offset + 2],
                pred_boxes[box_offset + 3],
            ],
            image_width,
            image_height,
        );
        detections.push(ComicTextBubbleRegion {
            label_id: class_id,
            label: label_name(class_id).to_string(),
            score,
            bbox,
        });
    }

    Ok(detections)
}

fn label_name(label_id: usize) -> &'static str {
    ID2LABEL.get(label_id).copied().unwrap_or("label")
}

fn detections_to_text_blocks(
    image_dimensions: (u32, u32),
    detections: &[ComicTextBubbleRegion],
) -> Vec<TextRegion> {
    let text_boxes = merge_text_regions(
        detections
            .iter()
            .filter(|region| region.is_text())
            .cloned()
            .collect::<Vec<_>>(),
    );

    let (image_width, image_height) = image_dimensions;
    let image_width = image_width as f32;
    let image_height = image_height as f32;
    let mut blocks = Vec::with_capacity(text_boxes.len());
    for text_region in text_boxes {
        let bbox = clamp_box(text_region.bbox, image_width, image_height);
        let width = (bbox[2] - bbox[0]).max(1.0);
        let height = (bbox[3] - bbox[1]).max(1.0);
        if width <= 5.0 || height <= 5.0 {
            continue;
        }

        let block = TextRegion {
            x: bbox[0],
            y: bbox[1],
            width,
            height,
            confidence: text_region.score,
            detector: Some(DETECTOR_NAME.to_string()),
            ..Default::default()
        };
        blocks.push(block);
    }
    blocks
}

fn filter_and_fix_regions(
    regions: Vec<ComicTextBubbleRegion>,
    image_dimensions: (u32, u32),
) -> Vec<ComicTextBubbleRegion> {
    let (image_width, image_height) = image_dimensions;
    let image_width = image_width as f32;
    let image_height = image_height as f32;
    regions
        .into_iter()
        .filter_map(|mut region| {
            region.bbox = clamp_box(region.bbox, image_width, image_height);
            let width = region.bbox[2] - region.bbox[0];
            let height = region.bbox[3] - region.bbox[1];
            if width <= 5.0 || height <= 5.0 {
                None
            } else {
                Some(region)
            }
        })
        .collect()
}

fn merge_text_regions(mut regions: Vec<ComicTextBubbleRegion>) -> Vec<ComicTextBubbleRegion> {
    let mut merged = Vec::new();
    while let Some(mut candidate) = regions.pop() {
        let mut index = 0usize;
        while index < regions.len() {
            let overlaps = rectangles_overlap(&candidate.bbox, &regions[index].bbox, 0.5)
                || is_mostly_contained(&candidate.bbox, &regions[index].bbox, 0.3)
                || is_mostly_contained(&regions[index].bbox, &candidate.bbox, 0.3);
            if overlaps {
                candidate.bbox = merge_boxes(candidate.bbox, regions[index].bbox);
                candidate.score = candidate.score.max(regions[index].score);
                regions.swap_remove(index);
            } else {
                index += 1;
            }
        }
        merged.push(candidate);
    }
    merged
}

fn merge_slice_regions(
    mut regions: Vec<ComicTextBubbleRegion>,
    image_height: u32,
) -> Vec<ComicTextBubbleRegion> {
    let y_distance_threshold = image_height as f32 * 0.1;
    let mut index = 0usize;
    while index < regions.len() {
        let mut compare = index + 1;
        while compare < regions.len() {
            if regions[index].label_id != regions[compare].label_id {
                compare += 1;
                continue;
            }

            let box1 = regions[index].bbox;
            let box2 = regions[compare].bbox;
            let area1 = box_area(box1);
            let area2 = box_area(box2);
            let iou = calculate_iou(&box1, &box2);
            let (contained, contains_first) = contained_relation(box1, box2, 0.85);

            if contained {
                if !contains_first {
                    regions[index].bbox = box2;
                }
                regions[index].score = regions[index].score.max(regions[compare].score);
                regions.swap_remove(compare);
                continue;
            }

            if iou >= 0.5 {
                if area2 > area1 {
                    regions[index].bbox = box2;
                }
                regions[index].score = regions[index].score.max(regions[compare].score);
                regions.swap_remove(compare);
                continue;
            }

            let width1 = (box1[2] - box1[0]).max(1.0);
            let height1 = (box1[3] - box1[1]).max(1.0);
            let width2 = (box2[2] - box2[0]).max(1.0);
            let height2 = (box2[3] - box2[1]).max(1.0);
            let y_dist = (box1[1] - box2[3]).abs().min((box1[3] - box2[1]).abs());
            let local_y_threshold = y_distance_threshold.min(height1.max(height2) * 0.1);
            let x_overlap = (box1[2].min(box2[2]) - box1[0].max(box2[0])).max(0.0);
            let x_overlap_ratio = x_overlap / width1.min(width2);
            let size_ratio = area1.min(area2) / area1.max(area2);

            if y_dist < local_y_threshold
                && x_overlap_ratio > 0.2
                && size_ratio > 0.3
                && (box1[0] - box2[0]).abs() < 0.5 * width1.max(width2)
                && (box1[2] - box2[2]).abs() < 0.5 * width1.max(width2)
            {
                let merged_box = merge_boxes(box1, box2);
                let merged_area = box_area(merged_box);
                if merged_area <= 3.0 * area1.max(area2) {
                    regions[index].bbox = merged_box;
                    regions[index].score = regions[index].score.max(regions[compare].score);
                    regions.swap_remove(compare);
                    continue;
                }
            }

            compare += 1;
        }
        index += 1;
    }
    regions
}

fn clamp_box(bbox: [f32; 4], image_width: f32, image_height: f32) -> [f32; 4] {
    [
        bbox[0].clamp(0.0, image_width),
        bbox[1].clamp(0.0, image_height),
        bbox[2].clamp(0.0, image_width),
        bbox[3].clamp(0.0, image_height),
    ]
}

fn scale_box_to_image(box_cxcywh: [f32; 4], image_width: f32, image_height: f32) -> [f32; 4] {
    let center_x = box_cxcywh[0] * image_width;
    let center_y = box_cxcywh[1] * image_height;
    let width = box_cxcywh[2] * image_width;
    let height = box_cxcywh[3] * image_height;
    [
        (center_x - width * 0.5).clamp(0.0, image_width),
        (center_y - height * 0.5).clamp(0.0, image_height),
        (center_x + width * 0.5).clamp(0.0, image_width),
        (center_y + height * 0.5).clamp(0.0, image_height),
    ]
}

fn sigmoid_scalar(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn calculate_iou(rect1: &[f32; 4], rect2: &[f32; 4]) -> f32 {
    let x1 = rect1[0].max(rect2[0]);
    let y1 = rect1[1].max(rect2[1]);
    let x2 = rect1[2].min(rect2[2]);
    let y2 = rect1[3].min(rect2[3]);
    let intersection_area = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union_area = box_area(*rect1) + box_area(*rect2) - intersection_area;
    if union_area <= 0.0 {
        0.0
    } else {
        intersection_area / union_area
    }
}

fn box_area(bbox: [f32; 4]) -> f32 {
    (bbox[2] - bbox[0]).max(0.0) * (bbox[3] - bbox[1]).max(0.0)
}

fn rectangles_overlap(rect1: &[f32; 4], rect2: &[f32; 4], threshold: f32) -> bool {
    calculate_iou(rect1, rect2) >= threshold
}

fn is_mostly_contained(outer: &[f32; 4], inner: &[f32; 4], threshold: f32) -> bool {
    let inner_area = box_area(*inner);
    if inner_area <= 0.0 || box_area(*outer) < inner_area {
        return false;
    }
    let x1 = outer[0].max(inner[0]);
    let y1 = outer[1].max(inner[1]);
    let x2 = outer[2].min(inner[2]);
    let y2 = outer[3].min(inner[3]);
    let overlap = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    overlap / inner_area >= threshold
}

fn contained_relation(box1: [f32; 4], box2: [f32; 4], threshold: f32) -> (bool, bool) {
    let area1 = box_area(box1);
    let area2 = box_area(box2);
    if area1 <= 0.0 || area2 <= 0.0 {
        return (false, false);
    }
    let x1 = box1[0].max(box2[0]);
    let y1 = box1[1].max(box2[1]);
    let x2 = box1[2].min(box2[2]);
    let y2 = box1[3].min(box2[3]);
    let overlap = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let containment_ratio = overlap / area1.min(area2);
    if containment_ratio < threshold {
        return (false, false);
    }
    (true, area1 >= area2)
}

fn merge_boxes(box1: [f32; 4], box2: [f32; 4]) -> [f32; 4] {
    [
        box1[0].min(box2[0]),
        box1[1].min(box2[1]),
        box1[2].max(box2[2]),
        box1[3].max(box2[3]),
    ]
}

#[derive(Debug, Clone)]
struct ImageSlicer {
    height_to_width_ratio_threshold: f32,
    target_slice_ratio: f32,
    overlap_height_ratio: f32,
    min_slice_height_ratio: f32,
}

impl Default for ImageSlicer {
    fn default() -> Self {
        Self {
            height_to_width_ratio_threshold: 3.5,
            target_slice_ratio: 3.0,
            overlap_height_ratio: 0.2,
            min_slice_height_ratio: 0.7,
        }
    }
}

impl ImageSlicer {
    fn should_slice(&self, image: &DynamicImage) -> bool {
        let (width, height) = image.dimensions();
        height as f32 / width.max(1) as f32 > self.height_to_width_ratio_threshold
    }

    fn calculate_slice_params(&self, image: &DynamicImage) -> (u32, u32, usize) {
        let (width, height) = image.dimensions();
        let slice_height = (width as f32 * self.target_slice_ratio).round().max(1.0) as u32;
        let effective_slice_height = (slice_height as f32 * (1.0 - self.overlap_height_ratio))
            .round()
            .max(1.0) as u32;
        let mut num_slices = height.div_ceil(effective_slice_height) as usize;
        if num_slices > 1 {
            let last_slice_start = (num_slices as u32 - 1) * effective_slice_height;
            let last_slice_height = height.saturating_sub(last_slice_start);
            if last_slice_height as f32 / slice_height as f32 <= self.min_slice_height_ratio {
                num_slices -= 1;
            }
        }
        (slice_height, effective_slice_height, num_slices.max(1))
    }

    fn process_slices_for_detection<F>(
        &self,
        image: &DynamicImage,
        detect_fn: F,
    ) -> Result<Vec<ComicTextBubbleRegion>>
    where
        F: Fn(&DynamicImage) -> Result<Vec<ComicTextBubbleRegion>>,
    {
        if !self.should_slice(image) {
            return detect_fn(image);
        }

        let (slice_height, effective_slice_height, num_slices) = self.calculate_slice_params(image);
        let (width, height) = image.dimensions();
        let mut detections = Vec::new();
        for slice_number in 0..num_slices {
            let start_y = slice_number as u32 * effective_slice_height;
            let end_y = if slice_number + 1 == num_slices {
                height
            } else {
                (start_y + slice_height).min(height)
            };
            let crop_height = end_y.saturating_sub(start_y).max(1);
            let cropped = image.crop_imm(0, start_y, width, crop_height);
            let mut slice_detections = detect_fn(&cropped)?;
            for detection in &mut slice_detections {
                detection.bbox[1] += start_y as f32;
                detection.bbox[3] += start_y as f32;
            }
            detections.extend(slice_detections);
        }
        Ok(detections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_slice_regions_merges_duplicates_per_label() {
        let regions = vec![
            ComicTextBubbleRegion {
                label_id: 0,
                label: "bubble".to_string(),
                score: 0.8,
                bbox: [10.0, 10.0, 50.0, 50.0],
            },
            ComicTextBubbleRegion {
                label_id: 0,
                label: "bubble".to_string(),
                score: 0.7,
                bbox: [12.0, 12.0, 48.0, 48.0],
            },
            ComicTextBubbleRegion {
                label_id: 2,
                label: "text_free".to_string(),
                score: 0.9,
                bbox: [100.0, 100.0, 140.0, 140.0],
            },
        ];
        let merged = merge_slice_regions(regions, 500);
        assert_eq!(merged.len(), 2);
    }
}
