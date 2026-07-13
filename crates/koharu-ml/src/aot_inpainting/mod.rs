//! BallonsTranslator-compatible AOT image inpainting.

mod model;
mod processor;

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, RgbImage};
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

use self::{model::Model, processor::Processor};

koharu_runtime::huggingface! {
    WEIGHTS => "mayocream/aot-inpainting" => "model.safetensors",
}

#[derive(Debug)]
pub struct AotInpainting {
    model: Model,
    processor: Processor,
}

impl AotInpainting {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into()?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve AOT inpainting weights")?;
        let mut model = Model::new(device);
        model
            .load_safetensors(&weights_path)
            .with_context(|| format!("failed to load {}", weights_path.display()))?;
        Ok(Self {
            model,
            processor: Processor::new(device),
        })
    }

    pub fn inference(&self, image: &DynamicImage, mask: &GrayImage) -> Result<RgbImage> {
        self.inference_with_max_side(image, mask, 2048)
    }

    pub fn inference_with_max_side(
        &self,
        image: &DynamicImage,
        mask: &GrayImage,
        max_side: u32,
    ) -> Result<RgbImage> {
        koharu_torch::no_grad(|| self.processor.call(&self.model, image, mask, max_side))
    }
}
