//! LaMa inference with IOPaint-compatible orchestration.

mod config;
mod model;
mod processor;

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, RgbImage};
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

pub use self::config::{HDStrategy, InpaintRequest};
use self::{config::FFCResNetGeneratorConfig, model::Model, processor::InpaintModel};

koharu_runtime::huggingface! {
    WEIGHTS => "mayocream/lama-manga" => "lama-manga.safetensors",
}

#[derive(Debug)]
pub struct LaMa {
    model: Model,
    processor: InpaintModel,
}

impl LaMa {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into()?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve LaMa weights")?;
        let mut model = Model::new(&FFCResNetGeneratorConfig::default(), device);
        model
            .load_safetensors(&weights_path)
            .context("failed to load LaMa safetensors")?;
        Ok(Self {
            model,
            processor: InpaintModel::new(device),
        })
    }

    pub fn inference(
        &self,
        image: &DynamicImage,
        mask: &GrayImage,
        config: &InpaintRequest,
    ) -> Result<RgbImage> {
        koharu_torch::no_grad(|| self.processor.call(&self.model, image, mask, config))
    }
}
