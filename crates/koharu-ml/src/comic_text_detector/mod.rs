mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

pub use self::processor::{ComicTextBlock, ComicTextDetection, ComicTextDetectionJson, Quad};

use self::{
    model::Model,
    processor::{postprocess, preprocess, rearranged_inference},
};

koharu_runtime::huggingface! {
    YOLO_WEIGHTS => "mayocream/comic-text-detector" => "yolo-v5.safetensors",
    UNET_WEIGHTS => "mayocream/comic-text-detector" => "unet.safetensors",
    DBNET_WEIGHTS => "mayocream/comic-text-detector" => "dbnet.safetensors",
}

#[derive(Debug)]
pub struct ComicTextDetector {
    device: Device,
    model: Model,
}

impl ComicTextDetector {
    pub async fn load(device: crate::Device) -> Result<Self> {
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

        let model = Model::new(device);
        model
            .load_safetensors(&yolo_path, &unet_path, &dbnet_path)
            .context("failed to load comic-text-detector safetensors")?;

        Ok(Self { device, model })
    }

    pub fn inference(&self, image: &DynamicImage) -> Result<ComicTextDetection> {
        koharu_torch::no_grad(|| {
            if let Some(detection) =
                rearranged_inference(image, self.device, |input| self.model.forward(input))?
            {
                return Ok(detection);
            }
            let input = preprocess(image, self.device)?;
            let outputs = self.model.forward(&input.pixel_values);
            postprocess(outputs, &input, image)
        })
    }
}
