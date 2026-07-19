mod config;
mod model;
mod processor;

use std::path::Path;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

pub use self::processor::Recognition;

use self::{config::Config, model::Model, processor::Processor};

koharu_runtime::huggingface! {
    WEIGHTS => "mayocream/coo-comic-onomatopoeia-safetensors" => "trba-rot-sar-hardroi-2d/model.safetensors",
}

/// COO's reported-best TRBA+2D comic onomatopoeia recognizer.
#[derive(Debug)]
pub struct ComicOnomatopoeiaRecognizer {
    device: Device,
    model: Model,
    processor: Processor,
}

impl ComicOnomatopoeiaRecognizer {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve COO TRBA+2D weights")?;
        Self::load_from_path(device, weights_path)
    }

    pub fn load_from_path(device: crate::Device, path: impl AsRef<Path>) -> Result<Self> {
        let device: Device = device.try_into()?;
        let config = Config::default();
        let processor = Processor::new(include_str!("character_set.txt"), &config)?;
        let mut model = Model::new(&config, device);
        model
            .load(path.as_ref())
            .with_context(|| format!("failed to load {}", path.as_ref().display()))?;
        Ok(Self {
            device,
            model,
            processor,
        })
    }

    pub fn inference(&self, image: &DynamicImage) -> Result<Recognition> {
        koharu_torch::no_grad(|| {
            let pixel_values = self.processor.preprocess(image, self.device)?;
            let logits = self.model.forward(&pixel_values);
            self.processor.postprocess(&logits)
        })
    }
}
