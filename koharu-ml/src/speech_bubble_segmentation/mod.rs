mod model;

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::ops::sigmoid;
use candle_transformers::object_detection::{Bbox, non_maximum_suppression};
use image::{
    DynamicImage, Rgb, RgbImage,
    imageops::{self, FilterType},
};
use koharu_runtime::RuntimeManager;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::{device, loading, probability_map::ProbabilityMap};

use self::model::{Multiples, YoloV8Seg, YoloV8SegOutputs};

const HF_REPO: &str = "mayocream/speech-bubble-segmentation";
const CONFIG_FILENAME: &str = "config.json";
const SAFETENSORS_FILENAME: &str = "model.safetensors";

koharu_runtime::declare_hf_model_package!(
    id: "model:speech-bubble-segmentation:config",
    repo: HF_REPO,
    file: CONFIG_FILENAME,
    bootstrap: false,
    order: 116,
);
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
    config: SpeechBubbleSegmentationConfig,
    device: Device,
}

#[derive(Debug, Clone)]
struct PreparedInput {
    pixel_values: Tensor,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechBubbleRegion {
    pub label_id: usize,
    pub label: String,
    pub score: f32,
    pub bbox: [f32; 4],
    pub area: u32,
}

#[derive(Debug, Clone)]
struct RawSpeechBubbleRegion {
    label_id: usize,
    label: String,
    score: f32,
    bbox: [f32; 4],
    mask_coefficients: Vec<f32>,
}

#[derive(Debug, Clone, Deserialize)]
struct SpeechBubbleSegmentationConfig {
    model_type: String,
    variant: String,
    input_size: u32,
    num_classes: usize,
    num_masks: usize,
    num_prototypes: usize,
    reg_max: usize,
    class_names: Vec<String>,
    default_confidence_threshold: f32,
    default_nms_threshold: f32,
    mask_threshold: f32,
    letterbox_color: u8,
}

impl SpeechBubbleSegmentationConfig {
    fn validate(&self) -> Result<()> {
        if self.model_type != "yolov8-seg" {
            bail!("unsupported speech bubble model type {}", self.model_type);
        }
        if self.variant != "m" {
            bail!("unsupported YOLOv8 segmentation variant {}", self.variant);
        }
        if self.input_size == 0 || !self.input_size.is_multiple_of(32) {
            bail!("invalid input_size {}", self.input_size);
        }
        if self.num_classes == 0 {
            bail!("num_classes must be positive");
        }
        if self.class_names.len() != self.num_classes {
            bail!(
                "expected {} class names, found {}",
                self.num_classes,
                self.class_names.len()
            );
        }
        if self.num_masks == 0 {
            bail!("num_masks must be positive");
        }
        if self.num_prototypes == 0 {
            bail!("num_prototypes must be positive");
        }
        if self.reg_max == 0 {
            bail!("reg_max must be positive");
        }
        Ok(())
    }
}

impl SpeechBubbleSegmentation {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        let (config_path, weights_path) = resolve_model_paths(runtime).await?;
        Self::load_from_paths(&config_path, &weights_path, cpu)
    }

    pub fn load_from_paths(
        config_path: impl AsRef<Path>,
        weights_path: impl AsRef<Path>,
        cpu: bool,
    ) -> Result<Self> {
        let device = device(cpu)?;
        let config = loading::read_json::<SpeechBubbleSegmentationConfig>(config_path.as_ref())
            .with_context(|| format!("failed to parse {}", config_path.as_ref().display()))?;
        config.validate()?;
        let multiples = variant_multiples(&config)?;
        let model = loading::load_mmaped_safetensors_path(weights_path.as_ref(), &device, |vb| {
            YoloV8Seg::load(
                vb,
                multiples,
                config.num_classes,
                config.num_masks,
                config.num_prototypes,
                config.reg_max,
            )
        })?;

        Ok(Self {
            model,
            config,
            device,
        })
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage) -> Result<SpeechBubbleSegmentationResult> {
        self.inference_with_thresholds(
            image,
            self.config.default_confidence_threshold,
            self.config.default_nms_threshold,
        )
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
        let prepared = self.preprocess(image)?;
        let preprocess_elapsed = preprocess_started.elapsed();

        let forward_started = Instant::now();
        let outputs = self.model.forward(&prepared.pixel_values)?;
        let forward_elapsed = forward_started.elapsed();

        let postprocess_started = Instant::now();
        let result = postprocess(
            &outputs,
            &prepared,
            &self.config,
            confidence_threshold,
            nms_threshold,
        )?;
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

    fn preprocess(&self, image: &DynamicImage) -> Result<PreparedInput> {
        let rgb = image.to_rgb8();
        let (original_width, original_height) = rgb.dimensions();
        let input_size = self.config.input_size;
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

        let mut letterboxed = RgbImage::from_pixel(
            input_size,
            input_size,
            Rgb([self.config.letterbox_color; 3]),
        );
        imageops::overlay(
            &mut letterboxed,
            &resized,
            i64::from(pad_x),
            i64::from(pad_y),
        );

        let pixel_values = Tensor::from_vec(
            letterboxed.into_raw(),
            (1, input_size as usize, input_size as usize, 3),
            &self.device,
        )?
        .permute((0, 3, 1, 2))?
        .to_dtype(DType::F32)?;
        let pixel_values = (pixel_values * (1.0 / 255.0))?;

        Ok(PreparedInput {
            pixel_values,
            original_width,
            original_height,
            resized_width,
            resized_height,
            pad_x,
            pad_y,
            scale,
        })
    }
}

pub async fn prefetch(runtime: &RuntimeManager) -> Result<()> {
    let _ = resolve_model_paths(runtime).await?;
    Ok(())
}

async fn resolve_model_paths(runtime: &RuntimeManager) -> Result<(PathBuf, PathBuf)> {
    let downloads = runtime.downloads();
    let config = downloads
        .huggingface_model(HF_REPO, CONFIG_FILENAME)
        .await
        .with_context(|| format!("failed to download {CONFIG_FILENAME} from {HF_REPO}"))?;
    let weights = downloads
        .huggingface_model(HF_REPO, SAFETENSORS_FILENAME)
        .await
        .with_context(|| format!("failed to download {SAFETENSORS_FILENAME} from {HF_REPO}"))?;
    Ok((config, weights))
}

fn variant_multiples(config: &SpeechBubbleSegmentationConfig) -> Result<Multiples> {
    match config.variant.as_str() {
        "m" => Ok(Multiples::m()),
        other => bail!("unsupported YOLOv8 segmentation variant {other}"),
    }
}

fn postprocess(
    outputs: &YoloV8SegOutputs,
    prepared: &PreparedInput,
    config: &SpeechBubbleSegmentationConfig,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> Result<SpeechBubbleSegmentationResult> {
    let pred = outputs.pred.to_device(&Device::Cpu)?.i(0)?;
    let proto = outputs.proto.to_device(&Device::Cpu)?.i(0)?;
    let raw_regions =
        extract_regions(&pred, prepared, config, confidence_threshold, nms_threshold)?;
    let mut probability_map =
        ProbabilityMap::zeros(prepared.original_width, prepared.original_height);
    let mask_probabilities = build_mask_probabilities(&proto, prepared, config, &raw_regions)?;

    let mut regions = Vec::with_capacity(raw_regions.len());
    for (region, mask) in raw_regions.iter().zip(mask_probabilities.iter()) {
        let area = merge_mask_into_probability_map(
            &mut probability_map,
            mask,
            region.bbox,
            config.mask_threshold,
        );
        regions.push(SpeechBubbleRegion {
            label_id: region.label_id,
            label: region.label.clone(),
            score: region.score,
            bbox: region.bbox,
            area,
        });
    }

    Ok(SpeechBubbleSegmentationResult {
        image_width: prepared.original_width,
        image_height: prepared.original_height,
        regions,
        probability_map,
    })
}

fn extract_regions(
    pred: &Tensor,
    prepared: &PreparedInput,
    config: &SpeechBubbleSegmentationConfig,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> Result<Vec<RawSpeechBubbleRegion>> {
    let (channels, anchors) = pred.dims2()?;
    let expected_channels = 4 + config.num_classes + config.num_masks;
    if channels != expected_channels {
        bail!(
            "unexpected prediction shape ({channels}, {anchors}), expected channel count {expected_channels}"
        );
    }

    let mut grouped: Vec<Vec<Bbox<Vec<f32>>>> = vec![Vec::new(); config.num_classes];
    for anchor_idx in 0..anchors {
        let values = pred.i((.., anchor_idx))?.to_vec1::<f32>()?;
        let class_scores = &values[4..4 + config.num_classes];
        let Some((label_id, &score)) = class_scores
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
        else {
            continue;
        };
        if score < confidence_threshold {
            continue;
        }

        let bbox = map_bbox_to_original(
            [
                values[0] - values[2] * 0.5,
                values[1] - values[3] * 0.5,
                values[0] + values[2] * 0.5,
                values[1] + values[3] * 0.5,
            ],
            prepared,
        );
        if bbox[2] <= bbox[0] || bbox[3] <= bbox[1] {
            continue;
        }

        grouped[label_id].push(Bbox {
            xmin: bbox[0],
            ymin: bbox[1],
            xmax: bbox[2],
            ymax: bbox[3],
            confidence: score,
            data: values[4 + config.num_classes..].to_vec(),
        });
    }

    non_maximum_suppression(&mut grouped, nms_threshold);

    let mut regions = Vec::new();
    for (label_id, bboxes) in grouped.into_iter().enumerate() {
        let label = config
            .class_names
            .get(label_id)
            .cloned()
            .unwrap_or_else(|| format!("class-{label_id}"));
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
    proto: &Tensor,
    prepared: &PreparedInput,
    config: &SpeechBubbleSegmentationConfig,
    regions: &[RawSpeechBubbleRegion],
) -> Result<Vec<Vec<f32>>> {
    if regions.is_empty() {
        return Ok(Vec::new());
    }

    let (num_masks, proto_h, proto_w) = proto.dims3()?;
    if num_masks != config.num_masks {
        bail!(
            "unexpected proto channel count {num_masks}, expected {}",
            config.num_masks
        );
    }

    let coefficients = regions
        .iter()
        .flat_map(|region| region.mask_coefficients.iter().copied())
        .collect::<Vec<_>>();
    let coeffs = Tensor::from_vec(
        coefficients,
        (regions.len(), config.num_masks),
        &Device::Cpu,
    )?;
    let proto_flat = proto.reshape((config.num_masks, proto_h * proto_w))?;
    let mut masks = coeffs
        .matmul(&proto_flat)?
        .reshape((regions.len(), 1, proto_h, proto_w))?;

    let (top, left, bottom, right) = mask_crop_window(
        prepared.original_width,
        prepared.original_height,
        proto_w as u32,
        proto_h as u32,
    );
    masks = masks.i((.., .., top..bottom, left..right))?;
    masks = masks.interpolate2d(
        prepared.original_height as usize,
        prepared.original_width as usize,
    )?;
    let masks = sigmoid(&masks.squeeze(1)?)?;

    let mut outputs = Vec::with_capacity(regions.len());
    for index in 0..regions.len() {
        outputs.push(masks.i(index)?.flatten_all()?.to_vec1::<f32>()?);
    }
    Ok(outputs)
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

fn merge_mask_into_probability_map(
    probability_map: &mut ProbabilityMap,
    mask: &[f32],
    bbox: [f32; 4],
    threshold: f32,
) -> u32 {
    let width = probability_map.width as usize;
    let height = probability_map.height as usize;
    if mask.len() != width * height {
        return 0;
    }

    let x1 = bbox[0].floor().clamp(0.0, probability_map.width as f32) as usize;
    let y1 = bbox[1].floor().clamp(0.0, probability_map.height as f32) as usize;
    let x2 = bbox[2].ceil().clamp(0.0, probability_map.width as f32) as usize;
    let y2 = bbox[3].ceil().clamp(0.0, probability_map.height as f32) as usize;
    if x2 <= x1 || y2 <= y1 {
        return 0;
    }

    let mut area = 0u32;
    for y in y1..y2.min(height) {
        let row_offset = y * width;
        for x in x1..x2.min(width) {
            let idx = row_offset + x;
            let value = mask[idx];
            if value >= threshold {
                area += 1;
            }
            if value > probability_map.values[idx] {
                probability_map.values[idx] = value;
            }
        }
    }
    area
}

#[cfg(test)]
mod tests {
    use super::{PreparedInput, map_bbox_to_original, mask_crop_window};
    use candle_core::{DType, Device, Tensor};

    #[test]
    fn map_bbox_to_original_removes_letterbox_padding() {
        let prepared = PreparedInput {
            pixel_values: Tensor::zeros((1, 3, 640, 640), DType::F32, &Device::Cpu)
                .expect("tensor"),
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
}
