//! PaddleOCR-VL 1.6 element recognition through llama.cpp and MTMD.

mod model;
mod processor;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_runtime::package::huggingface;

pub use self::processor::{PaddleOCRVLResult, PaddleOCRVLTask};

use self::{model::Model, processor::Processor};

koharu_runtime::huggingface! {
    WEIGHTS => "PaddlePaddle/PaddleOCR-VL-1.6-GGUF" => "PaddleOCR-VL-1.6-GGUF.gguf",
    MMPROJ => "PaddlePaddle/PaddleOCR-VL-1.6-GGUF" => "PaddleOCR-VL-1.6-GGUF-mmproj.gguf",
    CHAT_TEMPLATE => "PaddlePaddle/PaddleOCR-VL-1.6-GGUF" => "chat_template.jinja",
}

#[derive(Debug)]
pub struct PaddleOCRVL {
    model: Model,
    processor: Processor,
}

impl PaddleOCRVL {
    pub async fn load(device: crate::Device) -> Result<Self> {
        let (weights, mmproj, chat_template) = tokio::try_join!(
            huggingface::resolve(WEIGHTS),
            huggingface::resolve(MMPROJ),
            huggingface::resolve(CHAT_TEMPLATE),
        )?;
        let chat_template = tokio::fs::read_to_string(chat_template).await?;
        let (model, processor) = tokio::task::spawn_blocking(move || {
            Model::new(&device, weights, mmproj, chat_template)
        })
        .await
        .context("PaddleOCR-VL llama.cpp loading task panicked")??;
        Ok(Self { model, processor })
    }

    pub fn inference(
        &self,
        image: &DynamicImage,
        task: PaddleOCRVLTask,
    ) -> Result<PaddleOCRVLResult> {
        let bitmap = self.processor.bitmap(image)?;
        let prompt = self.processor.render_prompt(task)?;
        let text = self.model.forward(&bitmap, prompt, 512)?;
        Ok(PaddleOCRVLResult { text })
    }
}
