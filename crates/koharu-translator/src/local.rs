use async_trait::async_trait;
use std::sync::Arc;

use anyhow::Context;
use koharu_ml::llm::{
    ChatMessage, ChatTemplateOptions, GenerationOptions, Input, Llm, LoadOptions,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Device, Error, LocalModel, Result, Translation, TranslationRequest, Translator, prompt,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct LocalConfig {
    pub model: String,
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            model: "gemma4-12b-it".into(),
        }
    }
}

/// Loading and generation overrides for a local translator.
#[derive(Debug, Clone)]
pub struct LocalTranslatorOptions {
    pub gpu_layers: u32,
    pub use_mmap: bool,
    pub use_mlock: bool,
    /// Uses model-tuned defaults when omitted.
    pub generation: Option<GenerationOptions>,
}

impl Default for LocalTranslatorOptions {
    fn default() -> Self {
        let load = LoadOptions::default();
        Self {
            gpu_layers: load.gpu_layers,
            use_mmap: load.use_mmap,
            use_mlock: load.use_mlock,
            generation: None,
        }
    }
}

/// A downloaded GGUF translation model running through `koharu_ml::llm`.
#[derive(Debug)]
pub struct LocalTranslator {
    model: LocalModel,
    llm: Arc<Llm>,
    generation: GenerationOptions,
}

impl LocalTranslator {
    /// Resolves the catalog artifact and loads it on `device`.
    pub async fn load(device: Device, model: LocalModel) -> Result<Self> {
        Self::load_with_options(device, model, LocalTranslatorOptions::default()).await
    }

    pub async fn load_with_options(
        device: Device,
        model: LocalModel,
        options: LocalTranslatorOptions,
    ) -> Result<Self> {
        let model_path = model.resolve().await?;
        let load_options = LoadOptions {
            gpu_layers: options.gpu_layers,
            use_mmap: options.use_mmap,
            use_mlock: options.use_mlock,
            eos_token_id: None,
            mtmd: None,
        };
        let llm = Llm::load_with_options(device, model_path, load_options)
            .await
            .context("failed to load local translation model")?;
        Ok(Self {
            model,
            llm: Arc::new(llm),
            generation: options
                .generation
                .unwrap_or_else(|| model.generation_options()),
        })
    }

    #[must_use]
    pub fn model(&self) -> LocalModel {
        self.model
    }

    #[must_use]
    pub fn generation_options(&self) -> &GenerationOptions {
        &self.generation
    }
}

#[async_trait]
impl Translator for LocalTranslator {
    fn provider(&self) -> &'static str {
        "local"
    }

    async fn translate(&self, request: TranslationRequest) -> Result<Translation> {
        let expected = request.segments.len();
        if expected == 0 {
            return Ok(Translation {
                segments: Vec::new(),
            });
        }
        if !self
            .model
            .descriptor()
            .target_languages
            .contains(request.target_language)
        {
            return Err(Error::UnsupportedLanguage {
                provider: "local",
                language: request.target_language,
            });
        }

        let prompt = self.render_prompt(&request)?;
        let schema = prompt::output_schema(expected);
        let llm = Arc::clone(&self.llm);
        let generation = self.generation.clone();
        let output = tokio::task::spawn_blocking(move || {
            llm.inference_with_json_schema(&Input::new(&prompt), &generation, &schema)
        })
        .await
        .context("local translation task panicked")??;
        let segments = prompt::translations("local", &output.text, &request.segments)?;
        if segments.len() != expected {
            return Err(Error::SegmentCount {
                provider: "local",
                expected,
                actual: segments.len(),
            });
        }
        Ok(Translation { segments })
    }
}

impl LocalTranslator {
    fn render_prompt(&self, request: &TranslationRequest) -> Result<String> {
        let (system, payload) = prompt::prompts(request)?;
        Ok(self
            .llm
            .render_chat_prompt_with_options(
                &[ChatMessage::system(system), ChatMessage::user(payload)],
                ChatTemplateOptions {
                    add_generation_prompt: true,
                },
            )
            .context("failed to render local translation prompt")?)
    }
}
