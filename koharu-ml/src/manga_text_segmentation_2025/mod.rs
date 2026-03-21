mod model;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::ops::sigmoid;
use image::{DynamicImage, GrayImage};

use crate::{device, loading};

const REPO: &str = "mayocream/manga-text-segmentation-2025";
const SAFETENSORS_FILENAME: &str = "model.safetensors";
const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];

#[derive(Debug, Clone)]
pub struct ProbabilityMap {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

impl ProbabilityMap {
    pub fn to_gray_image(&self) -> Result<GrayImage> {
        let bytes = self
            .values
            .iter()
            .copied()
            .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect::<Vec<_>>();
        GrayImage::from_raw(self.width, self.height, bytes)
            .context("failed to build probability map image")
    }

    pub fn threshold(&self, threshold: f32) -> Result<GrayImage> {
        let bytes = self
            .values
            .iter()
            .copied()
            .map(|value| if value >= threshold { 255 } else { 0 })
            .collect::<Vec<_>>();
        GrayImage::from_raw(self.width, self.height, bytes).context("failed to build mask image")
    }

    pub fn max_value(&self) -> f32 {
        self.values.iter().copied().fold(0.0, f32::max)
    }
}

#[derive(Debug)]
pub struct MangaTextSegmentation {
    model: model::MangaTextSegmentationModel,
    device: Device,
    mean: Tensor,
    std: Tensor,
}

impl MangaTextSegmentation {
    pub async fn load(cpu: bool) -> Result<Self> {
        let safetensors = resolve_safetensors_path().await?;
        Self::load_from_path(&safetensors, cpu)
    }

    pub fn load_from_path(path: impl AsRef<Path>, cpu: bool) -> Result<Self> {
        let device = device(cpu)?;
        let model = loading::load_mmaped_safetensors_path(
            path.as_ref(),
            &device,
            model::MangaTextSegmentationModel::load,
        )?;
        let mean =
            Tensor::from_slice(&IMAGENET_MEAN, (1, 3, 1, 1), &device)?.to_dtype(DType::F32)?;
        let std = Tensor::from_slice(&IMAGENET_STD, (1, 3, 1, 1), &device)?.to_dtype(DType::F32)?;
        Ok(Self {
            model,
            device,
            mean,
            std,
        })
    }

    pub fn inference(&self, image: &DynamicImage) -> Result<ProbabilityMap> {
        let (pixel_values, width, height) = self.preprocess(image)?;
        let logits = self.model.forward(&pixel_values)?;
        let probabilities = sigmoid(&logits)?
            .i((0, 0, 0..height as usize, 0..width as usize))?
            .to_device(&Device::Cpu)?;
        let values = probabilities.flatten_all()?.to_vec1::<f32>()?;
        Ok(ProbabilityMap {
            width,
            height,
            values,
        })
    }

    fn preprocess(&self, image: &DynamicImage) -> Result<(Tensor, u32, u32)> {
        let rgb = image.to_rgb8();
        let (width, height) = rgb.dimensions();
        let pad_h = (32 - height % 32) % 32;
        let pad_w = (32 - width % 32) % 32;

        let tensor = Tensor::from_vec(
            rgb.into_raw(),
            (1, height as usize, width as usize, 3),
            &self.device,
        )?
        .permute((0, 3, 1, 2))?
        .to_dtype(DType::F32)?;
        let tensor = (tensor * (1.0 / 255.0))?
            .broadcast_sub(&self.mean)?
            .broadcast_div(&self.std)?;
        let tensor = tensor
            .pad_with_zeros(2, 0, pad_h as usize)?
            .pad_with_zeros(3, 0, pad_w as usize)?;

        Ok((tensor, width, height))
    }
}

pub async fn prefetch() -> Result<()> {
    let _ = resolve_safetensors_path().await?;
    Ok(())
}

async fn resolve_safetensors_path() -> Result<PathBuf> {
    koharu_http::download::model(REPO, SAFETENSORS_FILENAME)
        .await
        .with_context(|| format!("failed to download {SAFETENSORS_FILENAME} from {REPO}"))
}
