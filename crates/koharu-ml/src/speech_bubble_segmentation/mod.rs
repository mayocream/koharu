mod config;
mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

pub use self::{
    config::SpeechBubbleSegmentationConfig,
    processor::{
        SpeechBubbleRegion, SpeechBubbleRegionMask, SpeechBubbleSegmentationResult,
        YoloV8SegImageProcessor,
    },
};

use self::model::Model;

koharu_runtime::huggingface! {
    CONFIG => "mayocream/speech-bubble-segmentation" => "config.json",
    WEIGHTS => "mayocream/speech-bubble-segmentation" => "model.safetensors",
}

#[derive(Debug)]
pub struct SpeechBubbleSegmentation {
    device: Device,
    config: SpeechBubbleSegmentationConfig,
    model: Model,
    processor: YoloV8SegImageProcessor,
}

impl SpeechBubbleSegmentation {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into()?;
        let config_path = huggingface::resolve(CONFIG)
            .await
            .context("failed to resolve speech bubble segmentation config")?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve speech bubble segmentation weights")?;
        let config = SpeechBubbleSegmentationConfig::from_file(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let processor = YoloV8SegImageProcessor::new(&config)?;
        let mut model = Model::new(&config, device)?;
        model
            .load_safetensors(&weights_path)
            .with_context(|| format!("failed to load {}", weights_path.display()))?;
        Ok(Self {
            device,
            config,
            model,
            processor,
        })
    }

    pub fn inference(&self, image: &DynamicImage) -> Result<SpeechBubbleSegmentationResult> {
        self.inference_with_thresholds(
            image,
            self.config.default_confidence_threshold,
            self.config.default_nms_threshold,
        )
    }

    pub fn inference_with_thresholds(
        &self,
        image: &DynamicImage,
        confidence_threshold: f32,
        nms_threshold: f32,
    ) -> Result<SpeechBubbleSegmentationResult> {
        koharu_torch::no_grad(|| {
            let (pixel_values, letterbox) = self.processor.preprocess(image, self.device)?;
            let outputs = self.model.forward(&pixel_values);
            self.processor
                .postprocess(&outputs, &letterbox, confidence_threshold, nms_threshold)
        })
    }
}
