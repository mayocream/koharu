mod config;
mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

pub use self::{
    config::{ComicTextBubbleDetectorConfig, RtDetrResNetConfig},
    processor::{
        ComicTextBubbleBlock, ComicTextBubbleDetection, ComicTextBubbleProcessor,
        ComicTextBubbleRegion, ProcessorSize,
    },
};

use self::model::Model;

koharu_runtime::huggingface! {
    CONFIG => "ogkalu/comic-text-and-bubble-detector" => "config.json",
    PREPROCESSOR_CONFIG => "ogkalu/comic-text-and-bubble-detector" => "preprocessor_config.json",
    WEIGHTS => "ogkalu/comic-text-and-bubble-detector" => "model.safetensors",
}

#[derive(Debug)]
pub struct ComicTextBubbleDetector {
    device: Device,
    processor: ComicTextBubbleProcessor,
    model: Model,
}

impl ComicTextBubbleDetector {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into()?;
        let config_path = huggingface::resolve(CONFIG)
            .await
            .context("failed to resolve comic text/bubble detector config")?;
        let processor_path = huggingface::resolve(PREPROCESSOR_CONFIG)
            .await
            .context("failed to resolve comic text/bubble detector preprocessor config")?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve comic text/bubble detector weights")?;

        let mut config = ComicTextBubbleDetectorConfig::from_file(&config_path)
            .context("failed to parse comic text/bubble detector config")?;
        let processor = ComicTextBubbleProcessor::from_file(&processor_path)
            .context("failed to parse comic text/bubble detector preprocessor config")?
            .with_labels(config.labels());

        // The processor always produces this resolution, so the model can reuse the
        // fixed RT-DETR anchors instead of rebuilding and uploading them per slice.
        config.anchor_image_size = Some(vec![processor.size.height, processor.size.width]);

        let mut model = Model::new(config, device);
        model
            .load_safetensors(&weights_path)
            .context("failed to load comic text/bubble detector safetensors")?;

        Ok(Self {
            device,
            processor,
            model,
        })
    }

    pub fn inference(
        &self,
        image: &DynamicImage,
        confidence_threshold: f32,
    ) -> Result<ComicTextBubbleDetection> {
        koharu_torch::no_grad(|| {
            self.processor.inference_slices(image, |slice| {
                let input = self.processor.preprocess(slice, self.device);
                let outputs = self.model.forward(&input);
                self.processor
                    .postprocess(&outputs, slice, confidence_threshold)
            })
        })
    }
}
