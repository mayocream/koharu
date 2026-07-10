mod config;
mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

use crate::device;

pub use self::{
    config::{ComicTextBubbleDetectorConfig, RtDetrResNetConfig},
    processor::{
        ComicTextBubbleBlock, ComicTextBubbleDetection, ComicTextBubbleProcessor,
        ComicTextBubbleRegion, ProcessorSize,
    },
};

use self::model::ComicTextBubbleDetectorForObjectDetection;

koharu_runtime::huggingface! {
    CONFIG => "ogkalu/comic-text-and-bubble-detector" => "config.json",
    PREPROCESSOR_CONFIG => "ogkalu/comic-text-and-bubble-detector" => "preprocessor_config.json",
    WEIGHTS => "ogkalu/comic-text-and-bubble-detector" => "model.safetensors",
}

#[derive(Debug)]
pub struct ComicTextBubbleDetector {
    device: Device,
    processor: ComicTextBubbleProcessor,
    model: ComicTextBubbleDetectorForObjectDetection,
}

impl ComicTextBubbleDetector {
    pub async fn load(cpu: bool) -> Result<Self> {
        Self::load_with_threshold(cpu, 0.3).await
    }

    pub async fn load_with_threshold(cpu: bool, confidence_threshold: f32) -> Result<Self> {
        let device: Device = device(cpu).try_into()?;
        let config_path = huggingface::resolve(CONFIG)
            .await
            .context("failed to resolve comic text/bubble detector config")?;
        let processor_path = huggingface::resolve(PREPROCESSOR_CONFIG)
            .await
            .context("failed to resolve comic text/bubble detector preprocessor config")?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve comic text/bubble detector weights")?;

        let config = ComicTextBubbleDetectorConfig::from_file(&config_path)
            .context("failed to parse comic text/bubble detector config")?;
        let processor = ComicTextBubbleProcessor::from_file(&processor_path)
            .context("failed to parse comic text/bubble detector preprocessor config")?
            .with_labels(config.labels())
            .with_confidence_threshold(confidence_threshold);

        let mut model = ComicTextBubbleDetectorForObjectDetection::new(config, device);
        model
            .load_safetensors(&weights_path)
            .context("failed to load comic text/bubble detector safetensors")?;

        Ok(Self {
            device,
            processor,
            model,
        })
    }

    pub fn inference(&self, image: &DynamicImage) -> Result<ComicTextBubbleDetection> {
        koharu_torch::no_grad(|| {
            self.processor.inference_slices(image, |slice| {
                let input = self.processor.preprocess(slice, self.device);
                let outputs = self.model.forward(&input);
                self.processor.postprocess(&outputs, slice)
            })
        })
    }

    pub fn device(&self) -> Device {
        self.device
    }

    pub fn processor(&self) -> &ComicTextBubbleProcessor {
        &self.processor
    }
}
