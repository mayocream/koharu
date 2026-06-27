mod model;

use std::{path::Path, time::Instant};

use anyhow::{Context, Result, bail};
use burn::{
    module::{Module, ModuleMapper, Param},
    store::{ModuleSnapshot, PyTorchToBurnAdapter, SafetensorsStore},
    tensor::{
        DType, Device, DeviceKind, FloatDType, Tensor, TensorData,
        activation::sigmoid,
        module::interpolate,
        ops::{InterpolateMode, InterpolateOptions},
    },
};
use image::{
    DynamicImage, Rgb, RgbImage,
    imageops::{self, FilterType},
};
use koharu_runtime::RuntimeManager;
use serde::Serialize;
use tracing::instrument;

use crate::probability_map::ProbabilityMap;

use self::model::{INPUT_SIZE, NUM_CLASSES, NUM_MASKS, YoloV8Seg, YoloV8SegOutputs};

const HF_REPO: &str = "mayocream/speech-bubble-segmentation";
const SAFETENSORS_FILENAME: &str = "model.safetensors";
const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.25;
const DEFAULT_NMS_THRESHOLD: f32 = 0.45;
const MASK_THRESHOLD: f32 = 0.5;
const LETTERBOX_COLOR: u8 = 114;
const CLASS_NAMES: [&str; NUM_CLASSES] = ["speech bubble"];

koharu_runtime::declare_hf_model_package!(
    id: "model:speech-bubble-segmentation:weights",
    repo: HF_REPO,
    file: SAFETENSORS_FILENAME,
    bootstrap: false,
    order: 117,
);

#[derive(Debug)]
pub struct SpeechBubbleSegmentation {
    model: YoloV8Seg,
    device: Device,
    dtype: DType,
}

#[derive(Debug, Clone)]
struct PreparedInput {
    original_width: u32,
    original_height: u32,
    resized_width: u32,
    resized_height: u32,
    pad_x: u32,
    pad_y: u32,
    scale: f32,
}

#[derive(Debug, Clone)]
pub struct SpeechBubbleSegmentationResult {
    pub image_width: u32,
    pub image_height: u32,
    pub regions: Vec<SpeechBubbleRegion>,
    pub probability_map: ProbabilityMap,
}

#[derive(Debug, Clone)]
pub struct SpeechBubbleRegionMask {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl SpeechBubbleRegionMask {
    pub fn empty(x: u32, y: u32) -> Self {
        Self {
            x,
            y,
            width: 0,
            height: 0,
            pixels: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.pixels.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechBubbleRegion {
    pub label_id: usize,
    pub label: String,
    pub score: f32,
    pub bbox: [f32; 4],
    pub area: u32,
    #[serde(skip_serializing)]
    pub mask: SpeechBubbleRegionMask,
}

#[derive(Debug, Clone)]
struct RawSpeechBubbleRegion {
    label_id: usize,
    label: String,
    score: f32,
    bbox: [f32; 4],
    mask_coefficients: Vec<f32>,
}

impl SpeechBubbleSegmentation {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let weights_path = resolve_model_path(runtime).await?;
        Self::load_from_weights_path(&weights_path, cpu)
    }

    pub fn load_from_paths(
        _config_path: impl AsRef<Path>,
        weights_path: impl AsRef<Path>,
        cpu: bool,
    ) -> Result<Self> {
        Self::load_from_weights_path(weights_path, cpu)
    }

    pub fn load_from_weights_path(weights_path: impl AsRef<Path>, cpu: bool) -> Result<Self> {
        let (device, dtype, module_dtype) = make_device(cpu);
        let model = load_model(weights_path.as_ref(), &device, module_dtype)?;

        Ok(Self {
            model,
            device,
            dtype,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage) -> Result<SpeechBubbleSegmentationResult> {
        self.inference_with_thresholds(image, DEFAULT_CONFIDENCE_THRESHOLD, DEFAULT_NMS_THRESHOLD)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_thresholds(
        &self,
        image: &DynamicImage,
        confidence_threshold: f32,
        nms_threshold: f32,
    ) -> Result<SpeechBubbleSegmentationResult> {
        let started = Instant::now();
        let preprocess_started = Instant::now();
        let (pixel_values, prepared) = self.preprocess(image)?;
        let preprocess_elapsed = preprocess_started.elapsed();

        let forward_started = Instant::now();
        let outputs = self.model.forward(pixel_values);
        let forward_elapsed = forward_started.elapsed();

        let postprocess_started = Instant::now();
        let result = postprocess(outputs, &prepared, confidence_threshold, nms_threshold)?;
        let postprocess_elapsed = postprocess_started.elapsed();

        tracing::info!(
            width = image.width(),
            height = image.height(),
            resized_width = prepared.resized_width,
            resized_height = prepared.resized_height,
            detections = result.regions.len(),
            preprocess_ms = preprocess_elapsed.as_millis(),
            forward_ms = forward_elapsed.as_millis(),
            postprocess_ms = postprocess_elapsed.as_millis(),
            total_ms = started.elapsed().as_millis(),
            "speech bubble segmentation timings"
        );

        Ok(result)
    }

    fn preprocess(&self, image: &DynamicImage) -> Result<(Tensor<4>, PreparedInput)> {
        let rgb = image.to_rgb8();
        let (original_width, original_height) = rgb.dimensions();
        let input_size = INPUT_SIZE;
        let scale = f32::min(
            input_size as f32 / original_width.max(1) as f32,
            input_size as f32 / original_height.max(1) as f32,
        );
        let resized_width = ((original_width as f32 * scale).round() as u32).clamp(1, input_size);
        let resized_height = ((original_height as f32 * scale).round() as u32).clamp(1, input_size);
        let pad_x = (input_size - resized_width) / 2;
        let pad_y = (input_size - resized_height) / 2;

        let resized = if resized_width == original_width && resized_height == original_height {
            rgb
        } else {
            imageops::resize(&rgb, resized_width, resized_height, FilterType::Triangle)
        };

        let mut letterboxed =
            RgbImage::from_pixel(input_size, input_size, Rgb([LETTERBOX_COLOR; 3]));
        imageops::overlay(
            &mut letterboxed,
            &resized,
            i64::from(pad_x),
            i64::from(pad_y),
        );

        let input_size = input_size as usize;
        let plane = input_size * input_size;
        let rgb = letterboxed.into_raw();
        let mut data = vec![0.0_f32; 3 * plane];
        for (index, pixel) in rgb.chunks_exact(3).enumerate() {
            data[index] = pixel[0] as f32 / 255.0;
            data[plane + index] = pixel[1] as f32 / 255.0;
            data[2 * plane + index] = pixel[2] as f32 / 255.0;
        }
        let mut tensor_data = TensorData::new(data, [1, 3, input_size, input_size]);
        self.device.staging(std::iter::once(&mut tensor_data));
        let pixel_values = Tensor::from_data(tensor_data, (&self.device, self.dtype));

        Ok((
            pixel_values,
            PreparedInput {
                original_width,
                original_height,
                resized_width,
                resized_height,
                pad_x,
                pad_y,
                scale,
            },
        ))
    }
}

pub async fn prefetch(runtime: &RuntimeManager) -> Result<()> {
    let _ = resolve_model_path(runtime).await?;
    Ok(())
}

async fn resolve_model_path(runtime: &RuntimeManager) -> Result<std::path::PathBuf> {
    let downloads = runtime.downloads();
    downloads
        .huggingface_model(HF_REPO, SAFETENSORS_FILENAME)
        .await
        .with_context(|| format!("failed to download {SAFETENSORS_FILENAME} from {HF_REPO}"))
}

fn load_model(path: &Path, device: &Device, module_dtype: FloatDType) -> Result<YoloV8Seg> {
    let mut model = YoloV8Seg::new(device);
    let mut store = SafetensorsStore::from_file(path)
        .with_from_adapter(PyTorchToBurnAdapter)
        .with_key_remapping(r"^model\.0\.", "backbone.b1_0.")
        .with_key_remapping(r"^model\.1\.", "backbone.b1_1.")
        .with_key_remapping(r"^model\.2\.", "backbone.b2_0.")
        .with_key_remapping(r"^model\.3\.", "backbone.b2_1.")
        .with_key_remapping(r"^model\.4\.", "backbone.b2_2.")
        .with_key_remapping(r"^model\.5\.", "backbone.b3_0.")
        .with_key_remapping(r"^model\.6\.", "backbone.b3_1.")
        .with_key_remapping(r"^model\.7\.", "backbone.b4_0.")
        .with_key_remapping(r"^model\.8\.", "backbone.b4_1.")
        .with_key_remapping(r"^model\.9\.", "backbone.b5.")
        .with_key_remapping(r"^model\.12\.", "neck.n1.")
        .with_key_remapping(r"^model\.15\.", "neck.n2.")
        .with_key_remapping(r"^model\.16\.", "neck.n3.")
        .with_key_remapping(r"^model\.18\.", "neck.n4.")
        .with_key_remapping(r"^model\.19\.", "neck.n5.")
        .with_key_remapping(r"^model\.21\.", "neck.n6.")
        .with_key_remapping(r"^model\.22\.", "head.")
        .with_key_remapping(r"\.m\.", ".bottlenecks.")
        .with_key_remapping(r"^head\.cv([234])\.(\d+)\.0\.", "head.cv$1.$2.b0.")
        .with_key_remapping(r"^head\.cv([234])\.(\d+)\.1\.", "head.cv$1.$2.b1.")
        .with_key_remapping(r"^head\.cv([234])\.(\d+)\.2\.", "head.cv$1.$2.conv.")
        .skip_enum_variants(true)
        .allow_partial(false);
    let result = model
        .load_from(&mut store)
        .context("failed to mmap/load speech bubble segmentation safetensors through Burn store")?;
    if !result.errors.is_empty() {
        bail!(
            "failed to load speech bubble segmentation tensors: {}",
            result
        );
    }
    if !result.missing.is_empty() {
        bail!(
            "speech bubble segmentation checkpoint is missing tensors: {}",
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

fn postprocess(
    outputs: YoloV8SegOutputs,
    prepared: &PreparedInput,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> Result<SpeechBubbleSegmentationResult> {
    let raw_regions = extract_regions(outputs.pred, prepared, confidence_threshold, nms_threshold)?;
    let mut probability_map =
        ProbabilityMap::zeros(prepared.original_width, prepared.original_height);
    let mask_probabilities = build_mask_probabilities(outputs.proto, prepared, &raw_regions)?;

    let mut regions = Vec::with_capacity(raw_regions.len());
    for (region, mask) in raw_regions.iter().zip(mask_probabilities.iter()) {
        let (area, region_mask) =
            extract_region_contour_mask(&mut probability_map, mask, region.bbox, MASK_THRESHOLD)?;
        if area == 0 {
            continue;
        }
        regions.push(SpeechBubbleRegion {
            label_id: region.label_id,
            label: region.label.clone(),
            score: region.score,
            bbox: region.bbox,
            area,
            mask: region_mask,
        });
    }

    Ok(SpeechBubbleSegmentationResult {
        image_width: prepared.original_width,
        image_height: prepared.original_height,
        regions,
        probability_map,
    })
}

#[derive(Debug, Clone)]
struct DetectionBBox {
    xmin: f32,
    ymin: f32,
    xmax: f32,
    ymax: f32,
    confidence: f32,
    data: Vec<f32>,
}

fn extract_regions(
    pred: Tensor<3>,
    prepared: &PreparedInput,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> Result<Vec<RawSpeechBubbleRegion>> {
    let [batch, channels, anchors] = pred.dims();
    if batch != 1 {
        bail!("only single-image speech bubble inference is supported, got batch_size={batch}");
    }
    let expected_channels = 4 + NUM_CLASSES + NUM_MASKS;
    if channels != expected_channels {
        bail!(
            "unexpected prediction shape (1, {channels}, {anchors}), expected channel count {expected_channels}"
        );
    }

    let pred = tensor_to_f32_vec(pred)?;
    let mut grouped: Vec<Vec<DetectionBBox>> = vec![Vec::new(); NUM_CLASSES];
    for anchor_idx in 0..anchors {
        let mut label_id = 0usize;
        let mut score = f32::NEG_INFINITY;
        for class_id in 0..NUM_CLASSES {
            let value = pred[(4 + class_id) * anchors + anchor_idx];
            if value > score {
                label_id = class_id;
                score = value;
            }
        }
        if score < confidence_threshold {
            continue;
        }

        let center_x = pred[anchor_idx];
        let center_y = pred[anchors + anchor_idx];
        let width = pred[2 * anchors + anchor_idx];
        let height = pred[3 * anchors + anchor_idx];
        let bbox = map_bbox_to_original(
            [
                center_x - width * 0.5,
                center_y - height * 0.5,
                center_x + width * 0.5,
                center_y + height * 0.5,
            ],
            prepared,
        );
        if bbox[2] <= bbox[0] || bbox[3] <= bbox[1] {
            continue;
        }

        let mask_coefficients = (0..NUM_MASKS)
            .map(|index| pred[(4 + NUM_CLASSES + index) * anchors + anchor_idx])
            .collect::<Vec<_>>();
        grouped[label_id].push(DetectionBBox {
            xmin: bbox[0],
            ymin: bbox[1],
            xmax: bbox[2],
            ymax: bbox[3],
            confidence: score,
            data: mask_coefficients,
        });
    }

    non_maximum_suppression(&mut grouped, nms_threshold);

    let mut regions = Vec::new();
    for (label_id, bboxes) in grouped.into_iter().enumerate() {
        let label = CLASS_NAMES
            .get(label_id)
            .copied()
            .unwrap_or("speech bubble")
            .to_string();
        for bbox in bboxes {
            regions.push(RawSpeechBubbleRegion {
                label_id,
                label: label.clone(),
                score: bbox.confidence,
                bbox: [bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax],
                mask_coefficients: bbox.data,
            });
        }
    }
    regions.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(regions)
}

fn map_bbox_to_original(bbox: [f32; 4], prepared: &PreparedInput) -> [f32; 4] {
    let width = prepared.original_width as f32;
    let height = prepared.original_height as f32;
    let pad_x = prepared.pad_x as f32;
    let pad_y = prepared.pad_y as f32;
    [
        ((bbox[0] - pad_x) / prepared.scale).clamp(0.0, width),
        ((bbox[1] - pad_y) / prepared.scale).clamp(0.0, height),
        ((bbox[2] - pad_x) / prepared.scale).clamp(0.0, width),
        ((bbox[3] - pad_y) / prepared.scale).clamp(0.0, height),
    ]
}

fn build_mask_probabilities(
    proto: Tensor<4>,
    prepared: &PreparedInput,
    regions: &[RawSpeechBubbleRegion],
) -> Result<Vec<Vec<f32>>> {
    if regions.is_empty() {
        return Ok(Vec::new());
    }

    let [batch, num_masks, proto_h, proto_w] = proto.dims();
    if batch != 1 || num_masks != NUM_MASKS {
        bail!("unexpected proto shape [{batch}, {num_masks}, {proto_h}, {proto_w}]");
    }

    let coefficients = regions
        .iter()
        .flat_map(|region| region.mask_coefficients.iter().copied())
        .collect::<Vec<_>>();
    let device = proto.device();
    let dtype = proto.dtype();
    let coeffs = Tensor::from_data(
        TensorData::new(coefficients, [regions.len(), NUM_MASKS]),
        (&device, dtype),
    );
    let proto = proto.squeeze_dims::<3>(&[0]);
    let proto_flat = proto.reshape([NUM_MASKS, proto_h * proto_w]);
    let mut masks = coeffs
        .matmul(proto_flat)
        .reshape([regions.len(), 1, proto_h, proto_w]);

    let (top, left, bottom, right) = mask_crop_window(
        prepared.original_width,
        prepared.original_height,
        proto_w as u32,
        proto_h as u32,
    );
    masks = masks
        .narrow(2, top, bottom - top)
        .narrow(3, left, right - left);
    masks = interpolate(
        masks,
        [
            prepared.original_height as usize,
            prepared.original_width as usize,
        ],
        InterpolateOptions::new(InterpolateMode::Bilinear).with_align_corners(false),
    );
    let masks = sigmoid(masks.squeeze_dims::<3>(&[1]));

    let values = tensor_to_f32_vec(masks)?;
    let mask_len = prepared.original_width as usize * prepared.original_height as usize;
    Ok(values
        .chunks_exact(mask_len)
        .map(|chunk| chunk.to_vec())
        .collect())
}

fn non_maximum_suppression(bboxes: &mut [Vec<DetectionBBox>], threshold: f32) {
    for boxes in bboxes {
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
    let inter_w = (a.xmax.min(b.xmax) - a.xmin.max(b.xmin)).max(0.0);
    let inter_h = (a.ymax.min(b.ymax) - a.ymin.max(b.ymin)).max(0.0);
    let inter = inter_w * inter_h;
    if inter <= 0.0 {
        return 0.0;
    }

    let area_a = (a.xmax - a.xmin).max(0.0) * (a.ymax - a.ymin).max(0.0);
    let area_b = (b.xmax - b.xmin).max(0.0) * (b.ymax - b.ymin).max(0.0);
    inter / (area_a + area_b - inter).max(f32::EPSILON)
}

fn cast_module_float<M: Module>(module: M, dtype: FloatDType) -> M {
    struct CastMapper {
        dtype: FloatDType,
    }

    impl ModuleMapper for CastMapper {
        fn map_float<const D: usize>(&mut self, param: Param<Tensor<D>>) -> Param<Tensor<D>> {
            let (id, tensor, mapper) = param.consume();
            Param::from_mapped_value(id, tensor.cast(self.dtype), mapper)
        }
    }

    module.map(&mut CastMapper { dtype })
}

fn tensor_to_f32_vec<const D: usize>(tensor: Tensor<D>) -> Result<Vec<f32>> {
    tensor
        .cast(FloatDType::F32)
        .into_data()
        .into_vec::<f32>()
        .context("failed to extract burn tensor data as f32")
}

fn mask_crop_window(
    original_width: u32,
    original_height: u32,
    proto_width: u32,
    proto_height: u32,
) -> (usize, usize, usize, usize) {
    let gain = f32::min(
        proto_height as f32 / original_height.max(1) as f32,
        proto_width as f32 / original_width.max(1) as f32,
    );
    let pad_w = (proto_width as f32 - original_width as f32 * gain) / 2.0;
    let pad_h = (proto_height as f32 - original_height as f32 * gain) / 2.0;
    let top = ((pad_h - 0.1).round()).clamp(0.0, proto_height as f32) as usize;
    let left = ((pad_w - 0.1).round()).clamp(0.0, proto_width as f32) as usize;
    let bottom =
        proto_height as usize - ((pad_h + 0.1).round()).clamp(0.0, proto_height as f32) as usize;
    let right =
        proto_width as usize - ((pad_w + 0.1).round()).clamp(0.0, proto_width as f32) as usize;
    let bottom = bottom.max(top + 1).min(proto_height as usize);
    let right = right.max(left + 1).min(proto_width as usize);
    (top, left, bottom, right)
}

fn extract_region_contour_mask(
    probability_map: &mut ProbabilityMap,
    mask: &[f32],
    bbox: [f32; 4],
    threshold: f32,
) -> Result<(u32, SpeechBubbleRegionMask)> {
    let width = probability_map.width as usize;
    let height = probability_map.height as usize;
    if mask.len() != width * height {
        bail!(
            "speech bubble mask length {} does not match image area {}",
            mask.len(),
            width * height
        );
    }

    let x1 = bbox[0].floor().clamp(0.0, probability_map.width as f32) as usize;
    let y1 = bbox[1].floor().clamp(0.0, probability_map.height as f32) as usize;
    let x2 = bbox[2].ceil().clamp(0.0, probability_map.width as f32) as usize;
    let y2 = bbox[3].ceil().clamp(0.0, probability_map.height as f32) as usize;
    if x2 <= x1 || y2 <= y1 {
        return Ok((0, SpeechBubbleRegionMask::empty(x1 as u32, y1 as u32)));
    }

    let mask_width = x2 - x1;
    let mask_height = y2 - y1;
    let mut pixels = vec![0u8; mask_width * mask_height];
    let mut area = 0u32;
    for y in y1..y2.min(height) {
        let row_offset = y * width;
        let local_row_offset = (y - y1) * mask_width;
        for x in x1..x2.min(width) {
            let idx = row_offset + x;
            let value = mask[idx];
            if value >= threshold {
                area += 1;
                pixels[local_row_offset + (x - x1)] = u8::MAX;
            }
            if value > probability_map.values[idx] {
                probability_map.values[idx] = value;
            }
        }
    }
    Ok((
        area,
        SpeechBubbleRegionMask {
            x: x1 as u32,
            y: y1 as u32,
            width: mask_width as u32,
            height: mask_height as u32,
            pixels,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        PreparedInput, extract_region_contour_mask, map_bbox_to_original, mask_crop_window,
    };

    use crate::probability_map::ProbabilityMap;

    #[test]
    fn map_bbox_to_original_removes_letterbox_padding() {
        let prepared = PreparedInput {
            original_width: 1000,
            original_height: 500,
            resized_width: 640,
            resized_height: 320,
            pad_x: 0,
            pad_y: 160,
            scale: 0.64,
        };

        let bbox = map_bbox_to_original([100.0, 200.0, 540.0, 440.0], &prepared);
        assert!((bbox[0] - 156.25).abs() < 1e-3);
        assert!((bbox[1] - 62.5).abs() < 1e-3);
        assert!((bbox[2] - 843.75).abs() < 1e-3);
        assert!((bbox[3] - 437.5).abs() < 1e-3);
    }

    #[test]
    fn mask_crop_window_matches_letterboxed_square_input() {
        let (top, left, bottom, right) = mask_crop_window(1000, 500, 160, 160);
        assert_eq!((top, left, bottom, right), (40, 0, 120, 160));
    }

    #[test]
    fn extract_region_contour_mask_keeps_thresholded_shape() -> anyhow::Result<()> {
        let mut probability_map = ProbabilityMap::zeros(6, 5);
        let mut mask = vec![0.0f32; 6 * 5];
        mask[1 + 1 * 6] = 0.9;
        mask[2 + 1 * 6] = 0.8;
        mask[2 + 2 * 6] = 0.7;
        mask[4 + 3 * 6] = 0.4;

        let (area, region_mask) =
            extract_region_contour_mask(&mut probability_map, &mask, [1.0, 1.0, 5.0, 4.0], 0.5)?;

        assert_eq!(area, 3);
        assert_eq!((region_mask.x, region_mask.y), (1, 1));
        assert_eq!((region_mask.width, region_mask.height), (4, 3));
        assert_eq!(region_mask.pixels[0], u8::MAX);
        assert_eq!(region_mask.pixels[1], u8::MAX);
        assert_eq!(region_mask.pixels[5], u8::MAX);
        assert_eq!(region_mask.pixels[11], 0);
        assert_eq!(probability_map.values[4 + 3 * 6], 0.4);
        Ok(())
    }
}
