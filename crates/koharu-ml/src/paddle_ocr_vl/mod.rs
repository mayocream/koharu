//! PaddleOCR-VL 1.6 element recognition through llama.cpp and MTMD.

mod processor;

use crate::llm::{GenerationControl, GenerationOptions, Input, Llm, LoadOptions, MtmdOptions};
use anyhow::{Context, Result, ensure};
use image::DynamicImage;
use koharu_runtime::package::huggingface;

pub use self::processor::{PaddleOCRVLResult, PaddleOCRVLTask};

use self::processor::{Processor, repeated_suffix_start};

const PADDLEOCR_IMAGE_MARKER: &str = "<|IMAGE_START|><|IMAGE_PLACEHOLDER|><|IMAGE_END|>";

koharu_runtime::huggingface! {
    WEIGHTS => "PaddlePaddle/PaddleOCR-VL-1.6-GGUF" => "PaddleOCR-VL-1.6-GGUF.gguf",
    MMPROJ => "PaddlePaddle/PaddleOCR-VL-1.6-GGUF" => "PaddleOCR-VL-1.6-GGUF-mmproj.gguf",
    CHAT_TEMPLATE => "PaddlePaddle/PaddleOCR-VL-1.6-GGUF" => "chat_template.jinja",
}

#[derive(Debug)]
pub struct PaddleOCRVL {
    llm: Llm,
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
        let llm = Llm::load_with_options(
            device,
            weights,
            LoadOptions {
                mtmd: Some(MtmdOptions::new(mmproj).with_media_marker(PADDLEOCR_IMAGE_MARKER)),
                ..LoadOptions::default()
            },
        )
        .await
        .context("failed to load PaddleOCR-VL language model")?;
        ensure!(
            llm.capabilities().vision,
            "PaddleOCR-VL projector does not advertise vision support"
        );
        let processor = Processor::new(&llm, chat_template)?;
        Ok(Self { llm, processor })
    }

    pub fn inference(
        &self,
        image: &DynamicImage,
        task: PaddleOCRVLTask,
    ) -> Result<PaddleOCRVLResult> {
        let prompt = self.processor.render_prompt(task)?;
        let input = Input::new(&prompt).with_image(image);
        let options = GenerationOptions {
            max_tokens: 512,
            temperature: 0.0,
            repeat_penalty: 1.2,
            repeat_last_n: -1,
            ..GenerationOptions::default()
        };
        let generation = self
            .llm
            .inference_with_callback(&input, &options, |chunk| {
                Ok(if repeated_suffix_start(chunk.text).is_some() {
                    GenerationControl::Stop
                } else {
                    GenerationControl::Continue
                })
            })?;
        let mut text = generation.text;
        if let Some(trim_at) = repeated_suffix_start(&text) {
            text.truncate(trim_at);
        }
        let text = text.trim().to_owned();
        Ok(PaddleOCRVLResult { text })
    }
}
