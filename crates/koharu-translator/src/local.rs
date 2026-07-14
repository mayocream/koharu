use async_trait::async_trait;
use std::sync::Arc;

use anyhow::Context;
use koharu_ml::llm::{
    ChatMessage, ChatRole, ChatTemplateOptions, GenerationOptions, Input, Llm, LoadOptions,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Device, Error, Language, LocalModel, Result, Translation, TranslationContext,
    TranslationRequest, Translator,
};

const SAKURA_QWEN_CORRECT_EOS_ID: i32 = 151_645;

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
            eos_token_id: (model == LocalModel::Sakura1_5bQwen2_5v1_0)
                .then_some(SAKURA_QWEN_CORRECT_EOS_ID),
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
        let llm = Arc::clone(&self.llm);
        let generation = self.generation.clone();
        let output = tokio::task::spawn_blocking(move || {
            llm.inference_structured::<TranslationOutput>(&Input::new(&prompt), &generation)
        })
        .await
        .context("local translation task panicked")??
        .value;
        if output.translations.len() != expected {
            return Err(Error::SegmentCount {
                provider: "local",
                expected,
                actual: output.translations.len(),
            });
        }
        Ok(Translation {
            segments: output.translations,
        })
    }
}

impl LocalTranslator {
    fn render_prompt(&self, request: &TranslationRequest) -> Result<String> {
        // Model-specific message layouts ported from:
        // https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/prompt.rs
        let system = translation_system_prompt(request);
        let payload = serde_json::to_string(&TranslationInput {
            source_language: request.source_language,
            target_language: request.target_language,
            context: &request.context,
            segments: &request.segments,
        })
        .context("failed to serialize local translation input")?;

        let (messages, add_generation_prompt) = match self.model {
            LocalModel::VntlLlama3_8Bv2 => {
                let source = request.source_language.unwrap_or(Language::Japanese);
                (
                    vec![
                        ChatMessage::system(system),
                        ChatMessage::new(ChatRole::Named(source.to_string()), payload),
                        ChatMessage::new(
                            ChatRole::Named(request.target_language.to_string()),
                            String::new(),
                        ),
                    ],
                    false,
                )
            }
            LocalModel::HunyuanMT7B => (
                vec![ChatMessage::user(format!("{system}\n\n{payload}"))],
                true,
            ),
            _ => (
                vec![ChatMessage::system(system), ChatMessage::user(payload)],
                true,
            ),
        };

        let prompt = self
            .llm
            .render_chat_prompt_with_options(
                &messages,
                ChatTemplateOptions {
                    add_generation_prompt,
                },
            )
            .context("failed to render local translation prompt")?;
        if self.model == LocalModel::VntlLlama3_8Bv2 {
            Ok(prompt.trim_end_matches("<|eot_id|>").to_owned())
        } else {
            Ok(prompt)
        }
    }
}

#[derive(Serialize)]
struct TranslationInput<'a> {
    source_language: Option<Language>,
    target_language: Language,
    context: &'a [TranslationContext],
    segments: &'a [String],
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TranslationOutput {
    translations: Vec<String>,
}

pub(crate) fn translation_system_prompt(request: &TranslationRequest) -> String {
    let source = request
        .source_language
        .map(|language| language.to_string())
        .unwrap_or_else(|| "the detected source language".to_owned());
    let mut prompt = format!(
        "You are a professional manga translator. Translate every input segment from {source} into natural {}. Preserve character voice, emotional tone, relationship nuance, emphasis, and sound effects while keeping wording concise enough for speech bubbles. Return only a JSON object with one `translations` string for each input segment, in exactly the same order. Never merge, split, omit, or add segments.",
        request.target_language
    );
    if !request.context.is_empty() {
        prompt.push_str(
            " Use the supplied context only to preserve terminology, character voice, and dialogue continuity. Do not translate or return the context entries.",
        );
    }
    if let Some(instructions) = request
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|instructions| !instructions.is_empty())
    {
        prompt.push_str(" Additional instructions: ");
        prompt.push_str(instructions);
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_encodes_invariants_and_custom_instructions() {
        let request = TranslationRequest::new(["hello"], Language::Korean)
            .with_source_language(Language::Japanese)
            .with_instructions("Use informal speech.");
        let prompt = translation_system_prompt(&request);
        assert!(prompt.contains("from Japanese into natural Korean"));
        assert!(prompt.contains("same order"));
        assert!(prompt.contains("Use informal speech."));
    }

    #[test]
    fn empty_custom_instructions_are_ignored() {
        let request = TranslationRequest::new(["hello"], Language::English).with_instructions("  ");
        assert!(!translation_system_prompt(&request).contains("Additional instructions"));
    }

    #[test]
    fn context_is_reference_only() {
        let request = TranslationRequest::new(["Where is she?"], Language::Japanese)
            .with_context([TranslationContext::new("I saw Alice.", "アリスを見た。")]);
        let prompt = translation_system_prompt(&request);
        assert!(prompt.contains("dialogue continuity"));
        assert!(prompt.contains("Do not translate or return the context"));
    }
}
