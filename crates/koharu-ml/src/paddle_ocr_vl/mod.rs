//! PaddleOCR-VL-1.6 element recognition backed by the checkpoint revision
//! `66317acc4c9fc17bd154591ce650735cd2855f3e`.

mod config;
mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;
use koharu_torch::Device;

pub use self::{
    config::{PaddleOCRVLConfig, PaddleOCRVisionConfig, RopeScaling},
    processor::{PaddleOCRVLImageProcessor, PaddleOCRVLResult, PaddleOCRVLTask},
};

use self::{model::Model, processor::Processor};

koharu_runtime::huggingface! {
    CONFIG => "PaddlePaddle/PaddleOCR-VL-1.6" => "config.json",
    WEIGHTS => "PaddlePaddle/PaddleOCR-VL-1.6" => "model.safetensors",
    PROCESSOR => "PaddlePaddle/PaddleOCR-VL-1.6" => "preprocessor_config.json",
    TOKENIZER => "PaddlePaddle/PaddleOCR-VL-1.6" => "tokenizer.json",
}

#[derive(Debug)]
pub struct PaddleOCRVL {
    device: Device,
    model: Model,
    processor: Processor,
}

impl PaddleOCRVL {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let device: Device = device.try_into()?;
        let config_path = huggingface::resolve(CONFIG)
            .await
            .context("failed to resolve PaddleOCR-VL-1.6 config")?;
        let weights_path = huggingface::resolve(WEIGHTS)
            .await
            .context("failed to resolve PaddleOCR-VL-1.6 weights")?;
        let processor_path = huggingface::resolve(PROCESSOR)
            .await
            .context("failed to resolve PaddleOCR-VL-1.6 image processor")?;
        let tokenizer_path = huggingface::resolve(TOKENIZER)
            .await
            .context("failed to resolve PaddleOCR-VL-1.6 tokenizer")?;

        let config = PaddleOCRVLConfig::from_file(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let processor =
            Processor::from_files(&processor_path, &tokenizer_path, config.image_token_id)?;
        let mut model = Model::new(config, device);
        model
            .load_safetensors(&weights_path)
            .with_context(|| format!("failed to load {}", weights_path.display()))?;

        Ok(Self {
            device,
            model,
            processor,
        })
    }

    pub fn inference(
        &self,
        image: &DynamicImage,
        task: PaddleOCRVLTask,
    ) -> Result<PaddleOCRVLResult> {
        koharu_torch::no_grad(|| {
            let (pixel_values, image_grid_thw) =
                self.processor.preprocess(image, task, self.device)?;
            let (input_ids, mm_token_type_ids) =
                self.processor.encode_prompt(task, image_grid_thw)?;
            let token_ids = self.model.forward(
                &input_ids,
                &mm_token_type_ids,
                &pixel_values,
                image_grid_thw,
                512,
            )?;
            self.processor.decode(&token_ids)
        })
    }
}
