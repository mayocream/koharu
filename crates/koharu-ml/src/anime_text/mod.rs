mod model;

use std::{path::Path, path::PathBuf, time::Instant};

use anyhow::{Context, Result, bail};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_transformers::object_detection::{Bbox, non_maximum_suppression};
use image::{
    DynamicImage, Rgb, RgbImage,
    imageops::{self, FilterType},
};
use koharu_runtime::RuntimeManager;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::{device, loading, types::TextRegion};

use self::model::{Yolo12, Yolo12Scale};

pub const HF_REPO: &str = "mayocream/anime-text-yolo";
const INPUT_SIZE: u32 = 640;
const NUM_CLASSES: usize = 1;
const DEFAULT_VARIANT: AnimeTextYoloVariant = AnimeTextYoloVariant::N;
const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.25;
const DEFAULT_NMS_THRESHOLD: f32 = 0.45;
const LETTERBOX_COLOR: u8 = 114;
const DETECTOR_NAME: &str = "anime-text-yolo";
const CLASS_NAMES: [&str; NUM_CLASSES] = ["text_block"];

koharu_runtime::declare_hf_model_package!(
    id: "model:anime-text-yolo:yolo12n",
    repo: HF_REPO,
    file: "yolo12n_animetext.safetensors",
    bootstrap: false,
    order: 118,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:anime-text-yolo:yolo12s",
    repo: HF_REPO,
    file: "yolo12s_animetext.safetensors",
    bootstrap: false,
    order: 119,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:anime-text-yolo:yolo12m",
    repo: HF_REPO,
    file: "yolo12m_animetext.safetensors",
    bootstrap: false,
    order: 120,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:anime-text-yolo:yolo12l",
    repo: HF_REPO,
    file: "yolo12l_animetext.safetensors",
    bootstrap: false,
    order: 121,
);
koharu_runtime::declare_hf_model_package!(
    id: "model:anime-text-yolo:yolo12x",
    repo: HF_REPO,
    file: "yolo12x_animetext.safetensors",
    bootstrap: false,
    order: 122,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimeTextYoloVariant {
    N,
    S,
    M,
    L,
    X,
}

impl AnimeTextYoloVariant {
    pub fn filename(self) -> &'static str {
        match self {
            Self::N => "yolo12n_animetext.safetensors",
            Self::S => "yolo12s_animetext.safetensors",
            Self::M => "yolo12m_animetext.safetensors",
            Self::L => "yolo12l_animetext.safetensors",
            Self::X => "yolo12x_animetext.safetensors",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::N => "n",
            Self::S => "s",
            Self::M => "m",
            Self::L => "l",
            Self::X => "x",
        }
    }

    fn scale(self) -> Yolo12Scale {
        match self {
            Self::N => Yolo12Scale::N,
            Self::S => Yolo12Scale::S,
            Self::M => Yolo12Scale::M,
            Self::L => Yolo12Scale::L,
            Self::X => Yolo12Scale::X,
        }
    }
}

impl std::fmt::Display for AnimeTextYoloVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug)]
pub struct AnimeTextDetector {
    model: Yolo12,
    variant: AnimeTextYoloVariant,
    device: Device,
    dtype: DType,
}

#[derive(Debug, Clone)]
struct PreparedInput {
    pixel_values: Tensor,
    original_width: u32,
    original_height: u32,
    pad_x: u32,
    pad_y: u32,
    scale: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimeTextDetection {
    pub image_width: u32,
    pub image_height: u32,
    pub variant: AnimeTextYoloVariant,
    pub regions: Vec<AnimeTextRegion>,
    pub text_blocks: Vec<TextRegion>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimeTextRegion {
    pub label_id: usize,
    pub label: String,
    pub score: f32,
    pub bbox: [f32; 4],
}

impl AnimeTextDetector {
    pub async fn load(runtime: &RuntimeManager, cpu: bool) -> Result<Self> {
        Self::load_variant(runtime, DEFAULT_VARIANT, cpu).await
    }

    pub async fn load_variant(
        runtime: &RuntimeManager,
        variant: AnimeTextYoloVariant,
        cpu: bool,
    ) -> Result<Self> {
        let weights_path = resolve_model_path(runtime, variant).await?;
        Self::load_from_path(weights_path, variant, cpu)
    }

    pub fn load_from_path(
        weights_path: impl AsRef<Path>,
        variant: AnimeTextYoloVariant,
        cpu: bool,
    ) -> Result<Self> {
        let device = device(cpu)?;
        let dtype = loading::model_dtype(&device);
        let model = loading::load_mmaped_safetensors_path_with_dtype(
            weights_path.as_ref(),
            &device,
            dtype,
            |vb| Yolo12::load(vb, variant.scale(), NUM_CLASSES),
        )
        .with_context(|| {
            format!(
                "failed to load anime text YOLO {} weights from {}",
                variant,
                weights_path.as_ref().display()
            )
        })?;

        Ok(Self {
            model,
            variant,
            device,
            dtype,
        })
    }

    pub fn variant(&self) -> AnimeTextYoloVariant {
        self.variant
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference(&self, image: &DynamicImage) -> Result<AnimeTextDetection> {
        self.inference_with_thresholds(image, DEFAULT_CONFIDENCE_THRESHOLD, DEFAULT_NMS_THRESHOLD)
    }

    #[instrument(level = "debug", skip_all)]
    pub fn inference_with_thresholds(
        &self,
        image: &DynamicImage,
        confidence_threshold: f32,
        nms_threshold: f32,
    ) -> Result<AnimeTextDetection> {
        let started = Instant::now();
        let prepared = self.preprocess(image)?;
        let outputs = self.model.forward(&prepared.pixel_values)?;
        let regions = postprocess(&outputs, &prepared, confidence_threshold, nms_threshold)?;
        let text_blocks = regions_to_text_blocks(&regions);

        tracing::info!(
            width = image.width(),
            height = image.height(),
            variant = %self.variant,
            detections = regions.len(),
            total_ms = started.elapsed().as_millis(),
            "anime text YOLO timings"
        );

        Ok(AnimeTextDetection {
            image_width: prepared.original_width,
            image_height: prepared.original_height,
            variant: self.variant,
            regions,
            text_blocks,
        })
    }

    fn preprocess(&self, image: &DynamicImage) -> Result<PreparedInput> {
        let rgb = image.to_rgb8();
        let (original_width, original_height) = rgb.dimensions();
        let scale = f32::min(
            INPUT_SIZE as f32 / original_width.max(1) as f32,
            INPUT_SIZE as f32 / original_height.max(1) as f32,
        );
        let resized_width = ((original_width as f32 * scale).round() as u32).clamp(1, INPUT_SIZE);
        let resized_height = ((original_height as f32 * scale).round() as u32).clamp(1, INPUT_SIZE);
        let pad_x = (INPUT_SIZE - resized_width) / 2;
        let pad_y = (INPUT_SIZE - resized_height) / 2;

        let resized = if resized_width == original_width && resized_height == original_height {
            rgb
        } else {
            imageops::resize(&rgb, resized_width, resized_height, FilterType::Triangle)
        };

        let mut letterboxed =
            RgbImage::from_pixel(INPUT_SIZE, INPUT_SIZE, Rgb([LETTERBOX_COLOR; 3]));
        imageops::overlay(
            &mut letterboxed,
            &resized,
            i64::from(pad_x),
            i64::from(pad_y),
        );

        let pixel_values = Tensor::from_vec(
            letterboxed.into_raw(),
            (1, INPUT_SIZE as usize, INPUT_SIZE as usize, 3),
            &self.device,
        )?
        .permute((0, 3, 1, 2))?
        .to_dtype(self.dtype)?;
        let pixel_values = (pixel_values * (1.0 / 255.0))?;

        Ok(PreparedInput {
            pixel_values,
            original_width,
            original_height,
            pad_x,
            pad_y,
            scale,
        })
    }
}

pub async fn prefetch(runtime: &RuntimeManager) -> Result<()> {
    prefetch_variant(runtime, DEFAULT_VARIANT).await
}

pub async fn prefetch_variant(
    runtime: &RuntimeManager,
    variant: AnimeTextYoloVariant,
) -> Result<()> {
    let _ = resolve_model_path(runtime, variant).await?;
    Ok(())
}

async fn resolve_model_path(
    runtime: &RuntimeManager,
    variant: AnimeTextYoloVariant,
) -> Result<PathBuf> {
    runtime
        .downloads()
        .huggingface_model(HF_REPO, variant.filename())
        .await
        .with_context(|| format!("failed to download {} from {}", variant.filename(), HF_REPO))
}

fn postprocess(
    outputs: &Tensor,
    prepared: &PreparedInput,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> Result<Vec<AnimeTextRegion>> {
    let pred = outputs
        .to_dtype(DType::F32)?
        .to_device(&Device::Cpu)?
        .i(0)?;
    let (channels, anchors) = pred.dims2()?;
    let expected_channels = 4 + NUM_CLASSES;
    if channels != expected_channels {
        bail!(
            "unexpected anime text YOLO prediction channels {channels}, expected {expected_channels}"
        );
    }

    let mut grouped: Vec<Vec<Bbox<usize>>> = (0..NUM_CLASSES).map(|_| Vec::new()).collect();
    for anchor_idx in 0..anchors {
        let values = pred.i((.., anchor_idx))?.to_vec1::<f32>()?;
        let class_scores = &values[4..4 + NUM_CLASSES];
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
            data: label_id,
        });
    }

    non_maximum_suppression(&mut grouped, nms_threshold);

    let mut regions = Vec::new();
    for (label_id, bboxes) in grouped.into_iter().enumerate() {
        let label = CLASS_NAMES
            .get(label_id)
            .copied()
            .unwrap_or("text_block")
            .to_string();
        for bbox in bboxes {
            regions.push(AnimeTextRegion {
                label_id,
                label: label.clone(),
                score: bbox.confidence,
                bbox: [bbox.xmin, bbox.ymin, bbox.xmax, bbox.ymax],
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

fn regions_to_text_blocks(regions: &[AnimeTextRegion]) -> Vec<TextRegion> {
    regions
        .iter()
        .filter_map(|region| {
            let width = (region.bbox[2] - region.bbox[0]).max(0.0);
            let height = (region.bbox[3] - region.bbox[1]).max(0.0);
            if width <= 1.0 || height <= 1.0 {
                return None;
            }
            Some(TextRegion {
                x: region.bbox[0],
                y: region.bbox[1],
                width,
                height,
                confidence: region.score,
                detector: Some(DETECTOR_NAME.to_string()),
                ..Default::default()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{PreparedInput, map_bbox_to_original};
    use candle_core::{DType, Device, Tensor};

    #[test]
    fn map_bbox_to_original_removes_letterbox_padding() {
        let prepared = PreparedInput {
            pixel_values: Tensor::zeros((1, 3, 640, 640), DType::F32, &Device::Cpu)
                .expect("tensor"),
            original_width: 1000,
            original_height: 500,
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
}
