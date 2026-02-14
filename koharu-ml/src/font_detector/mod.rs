use std::{fs, path::PathBuf};

use crate::{define_models, device};
use anyhow::{Context, Result};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{
    VarBuilder,
    ops::{sigmoid, softmax},
};
use image::{DynamicImage, GenericImageView, imageops::FilterType};

mod models;
pub use models::ModelKind;

const FONT_COUNT: usize = 6_150;
const REGRESSION_START: usize = FONT_COUNT + 2;
const REGRESSION_DIM: usize = 10;

define_models! {
    FontWeights => ("fffonion/yuzumarker-font-detection", "yuzumarker-font-detection.safetensors"),
    FontNames => ("fffonion/yuzumarker-font-detection", "font-labels-ex.json"),
}

pub use koharu_types::{FontPrediction, NamedFontPrediction, TextDirection};

pub struct FontDetector {
    model: models::Model,
    labels: FontLabels,
    device: Device,
}

impl FontDetector {
    pub async fn load(use_cpu: bool) -> Result<Self> {
        Self::load_with_kind(use_cpu, ModelKind::default()).await
    }

    pub async fn load_with_kind(use_cpu: bool, kind: ModelKind) -> Result<Self> {
        let device = device(use_cpu)?;
        let weights = Manifest::FontWeights.get().await?;
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device)?
                .pp("model._orig_mod.model")
        };
        let model = models::Model::load(vb, kind)?;
        let labels = FontLabels::load().await?;

        Ok(Self {
            model,
            device,
            labels,
        })
    }

    pub fn inference(&self, images: &[DynamicImage], top_k: usize) -> Result<Vec<FontPrediction>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }

        let mut processed = Vec::with_capacity(images.len());
        let mut original_sizes = Vec::with_capacity(images.len());
        let input_size = self.model.input_size();
        for image in images {
            let (w, _h) = image.dimensions();
            original_sizes.push(w);
            processed.push(preprocess_image(image, input_size, &self.device)?);
        }
        let batch = Tensor::stack(&processed, 0)?;
        let logits = self.model.forward(&batch, false)?;

        let mut predictions = Vec::with_capacity(images.len());
        for (index, width) in original_sizes.into_iter().enumerate() {
            let example = logits.i(index)?;
            let font_logits = example.narrow(0, 0, FONT_COUNT)?;
            let font_probs = softmax(&font_logits, 0)?;
            let font_probs_vec: Vec<f32> = font_probs.to_vec1()?;
            let mut ranked: Vec<(usize, f32)> = font_probs_vec.into_iter().enumerate().collect();
            ranked.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            ranked.truncate(top_k.min(FONT_COUNT));

            let named_fonts = ranked
                .iter()
                .filter_map(|(idx, prob)| {
                    self.labels.entry(*idx).map(|label| NamedFontPrediction {
                        index: *idx,
                        name: label.name.clone(),
                        language: label.language.clone(),
                        probability: *prob,
                        serif: label.serif,
                    })
                })
                .collect();

            let direction_logits = example.narrow(0, FONT_COUNT, 2)?;
            let direction_vec: Vec<f32> = direction_logits.to_vec1()?;
            let direction = if direction_vec.len() == 2 && direction_vec[1] > direction_vec[0] {
                TextDirection::Vertical
            } else {
                TextDirection::Horizontal
            };

            let regression = example.narrow(0, REGRESSION_START, REGRESSION_DIM)?;
            // Regression head is trained on normalized values; bring logits into [0, 1].
            let regression = sigmoid(&regression)?;
            let mut regression: Vec<f32> = regression.to_vec1()?;
            regression.resize(REGRESSION_DIM, 0.0);
            let clamp01 = |v: f32| v.clamp(0.0, 1.0);
            let text_color = [
                (clamp01(regression[0]) * 255.0).round() as u8,
                (clamp01(regression[1]) * 255.0).round() as u8,
                (clamp01(regression[2]) * 255.0).round() as u8,
            ];
            let font_size_px = clamp01(regression[3]) * width as f32;
            let stroke_width_px = clamp01(regression[4]) * width as f32;
            let stroke_color = [
                (clamp01(regression[5]) * 255.0).round() as u8,
                (clamp01(regression[6]) * 255.0).round() as u8,
                (clamp01(regression[7]) * 255.0).round() as u8,
            ];
            let line_spacing_px = clamp01(regression[8]) * width as f32;
            let line_height = if font_size_px > 0.0 {
                1.0 + line_spacing_px / font_size_px
            } else {
                1.2
            };
            let angle_deg = (regression[9] - 0.5) * 180.0;

            predictions.push(FontPrediction {
                top_fonts: ranked,
                named_fonts,
                direction,
                text_color,
                stroke_color,
                font_size_px,
                stroke_width_px,
                line_height,
                angle_deg,
            });
        }

        Ok(predictions)
    }
}

#[derive(Debug, Clone)]
pub struct FontLabel {
    pub name: String,
    pub language: Option<String>,
    pub serif: bool,
}

#[derive(Debug, Clone)]
pub struct FontLabels {
    labels: Vec<FontLabel>,
}

impl FontLabels {
    pub async fn load() -> Result<Self> {
        let path = Manifest::FontNames.get().await?;
        Self::from_path(&path)
    }

    pub fn from_path(path: &PathBuf) -> Result<Self> {
        let data = fs::read_to_string(path)
            .with_context(|| format!("Failed to read labels file {}", path.display()))?;
        let entries: Vec<FontLabelEntry> = serde_json::from_str(&data)
            .with_context(|| format!("Failed to parse labels file {}", path.display()))?;
        let mut labels = Vec::with_capacity(entries.len());
        for entry in entries {
            labels.push(FontLabel {
                name: entry.path,
                language: entry.language,
                serif: entry.serif,
            });
        }
        Ok(Self { labels })
    }

    pub fn entry(&self, idx: usize) -> Option<&FontLabel> {
        self.labels.get(idx)
    }

    pub fn name(&self, idx: usize) -> Option<&str> {
        self.entry(idx).map(|label| label.name.as_str())
    }

    pub fn language(&self, idx: usize) -> Option<&str> {
        self.entry(idx).and_then(|label| label.language.as_deref())
    }
}

#[derive(serde::Deserialize)]
struct FontLabelEntry {
    path: String,
    language: Option<String>,
    serif: bool,
}

fn preprocess_image(image: &DynamicImage, target: usize, device: &Device) -> Result<Tensor> {
    let resized = image.resize_exact(target as u32, target as u32, FilterType::CatmullRom);
    let data = resized.to_rgb8().into_raw();
    let tensor = Tensor::from_vec(
        data,
        (target, target, 3),
        &Device::Cpu,
    )?
    .to_dtype(DType::F32)?
    .permute((2, 0, 1))? // (3, H, W)
    * (1.0 / 255.0);
    let tensor = tensor?;
    Ok(tensor.to_device(device)?)
}
