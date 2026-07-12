mod config;
mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

pub use self::{
    config::PPOCRV6MediumDetConfig,
    processor::{PPOCRV6MediumDetImageProcessor, TextDetection, TextDetections},
};

use self::model::Model;

koharu_runtime::huggingface! {
    CONFIG => "PaddlePaddle/PP-OCRv6_medium_det_safetensors" => "config.json",
    WEIGHTS => "PaddlePaddle/PP-OCRv6_medium_det_safetensors" => "model.safetensors",
    PROCESSOR => "PaddlePaddle/PP-OCRv6_medium_det_safetensors" => "preprocessor_config.json",
}

#[derive(Debug)]
pub struct PPOCRV6MediumDet {
    device: Device,
    model: Model,
    processor: PPOCRV6MediumDetImageProcessor,
}

impl PPOCRV6MediumDet {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into()?;
        let config_path = huggingface::resolve(CONFIG)
            .await
            .context("failed to resolve PP-OCRv6 medium detection config")?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve PP-OCRv6 medium detection weights")?;
        let processor_path = huggingface::resolve(PROCESSOR)
            .await
            .context("failed to resolve PP-OCRv6 medium detection image processor")?;

        let config = PPOCRV6MediumDetConfig::from_file(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let processor = PPOCRV6MediumDetImageProcessor::from_file(&processor_path)
            .with_context(|| format!("failed to read {}", processor_path.display()))?;
        let mut model = Model::new(&config, device);
        model
            .load_safetensors(&weights_path)
            .with_context(|| format!("failed to load {}", weights_path.display()))?;

        Ok(Self {
            device,
            model,
            processor,
        })
    }

    pub fn inference(&self, image: &DynamicImage) -> Result<TextDetections> {
        koharu_torch::no_grad(|| {
            let pixel_values = self.processor.preprocess(image, self.device)?;
            let output = self.model.forward(&pixel_values);
            self.processor.postprocess(&output, image)
        })
    }
}

#[cfg(test)]
mod tests {
    use koharu_runtime::package::{PreloadablePackage, libtorch::Libtorch};

    use super::*;

    #[tokio::test]
    #[ignore = "downloads the checkpoint and LibTorch runtime"]
    async fn loads_medium_detector_checkpoint() {
        Libtorch::for_current_target()
            .unwrap()
            .preload()
            .await
            .unwrap();
        let model = PPOCRV6MediumDet::load(crate::Device::cpu()).await.unwrap();
        model.inference(&DynamicImage::new_rgb8(64, 64)).unwrap();
    }
}
