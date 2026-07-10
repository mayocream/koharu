mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

pub use self::processor::{
    ComicTextBlock, ComicTextDetection, ComicTextDetectionJson, ComicTextDetectorConfig, Quad,
    threshold_mask,
};

use self::{
    model::ComicTextDetectorModel,
    processor::{postprocess, preprocess},
};

koharu_runtime::huggingface! {
    YOLO_WEIGHTS => "mayocream/comic-text-detector" => "yolo-v5.safetensors",
    UNET_WEIGHTS => "mayocream/comic-text-detector" => "unet.safetensors",
    DBNET_WEIGHTS => "mayocream/comic-text-detector" => "dbnet.safetensors",
}

#[derive(Debug)]
pub struct ComicTextDetector {
    device: Device,
    config: ComicTextDetectorConfig,
    model: ComicTextDetectorModel,
}

impl ComicTextDetector {
    pub async fn load(device: crate::Device) -> Result<Self> {
        Self::load_with_config(device, ComicTextDetectorConfig::default()).await
    }

    pub async fn load_with_config(
        device: crate::Device,
        config: ComicTextDetectorConfig,
    ) -> Result<Self> {
        let device: Device = device.try_into()?;
        let yolo_path = huggingface::resolve(YOLO_WEIGHTS)
            .await
            .context("failed to resolve comic-text-detector YOLO weights")?;
        let unet_path = huggingface::resolve(UNET_WEIGHTS)
            .await
            .context("failed to resolve comic-text-detector U-Net weights")?;
        let dbnet_path = huggingface::resolve(DBNET_WEIGHTS)
            .await
            .context("failed to resolve comic-text-detector DBNet weights")?;

        let model = ComicTextDetectorModel::new(device);
        model
            .load_safetensors(&yolo_path, &unet_path, &dbnet_path)
            .context("failed to load comic-text-detector safetensors")?;

        Ok(Self {
            device,
            config,
            model,
        })
    }

    pub fn inference(&self, image: &DynamicImage) -> Result<ComicTextDetection> {
        koharu_torch::no_grad(|| {
            let input = preprocess(image, self.device, &self.config)?;
            let outputs = self.model.forward(&input.pixel_values);
            postprocess(outputs, &input, &self.config)
        })
    }
}
