//! A segment-preserving translation interface for local GGUF models and hosted APIs.
//!
//! ```no_run
//! use koharu_translator::{
//!     Language, OpenAiConfig, RemoteProvider, RemoteTranslator, TranslationContext,
//!     TranslationRequest, Translator,
//! };
//!
//! # async fn example() -> koharu_translator::Result<()> {
//! koharu_secrets::set("openai", &koharu_secrets::SecretString::from("api-key"))?;
//! let translator = RemoteTranslator::new(RemoteProvider::OpenAi(OpenAiConfig::new(
//!     "gpt-4.1-mini",
//! )));
//! let translation = translator
//!     .translate(
//!         TranslationRequest::new(["おはよう", "行こう！"], Language::English).with_context([
//!             TranslationContext::new("また明日。", "See you tomorrow."),
//!         ]),
//!     )
//!     .await?;
//! assert_eq!(translation.segments.len(), 2);
//! # Ok(())
//! # }
//! ```

mod catalog;
mod config;
mod credentials;
mod json;
mod language;
mod local;
mod prompt;
mod remote;

use async_trait::async_trait;
use thiserror::Error;

pub use catalog::{
    LocalModel, LocalModelDescriptor, RemoteModelDescriptor, RemoteProviderDescriptor,
    RemoteProviderKind, SupportedLanguages, local_models, remote_models, remote_providers,
};
pub use config::{Providers, TranslationConfig};
pub use credentials::TranslationCredentials;
pub use koharu_ml::Device;
pub use koharu_ml::llm::GenerationOptions as LocalGenerationOptions;
pub use language::Language;
pub use local::{LocalConfig, LocalTranslator, LocalTranslatorOptions};
pub use remote::{
    CaiyunConfig, ClaudeConfig, DeepLConfig, DeepSeekConfig, GeminiConfig, GoogleCloudConfig,
    LmStudioConfig, OpenAiCompatibleConfig, OpenAiConfig, OpenRouterConfig,
    RemoteGenerationOptions, RemoteProvider, RemoteTranslator, discover_lm_studio_models,
    discover_openai_compatible_models, discover_openrouter_models,
};

pub type Result<T> = std::result::Result<T, Error>;

/// One translation job. Segment boundaries are preserved in the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationRequest {
    pub segments: Vec<String>,
    pub source_language: Option<Language>,
    pub target_language: Language,
    pub instructions: Option<String>,
    /// Earlier source/translation pairs, ordered from oldest to newest.
    pub context: Vec<TranslationContext>,
}

impl TranslationRequest {
    #[must_use]
    pub fn new(
        segments: impl IntoIterator<Item = impl Into<String>>,
        target_language: Language,
    ) -> Self {
        Self {
            segments: segments.into_iter().map(Into::into).collect(),
            source_language: None,
            target_language,
            instructions: None,
            context: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_source_language(mut self, source_language: Language) -> Self {
        self.source_language = Some(source_language);
        self
    }

    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    #[must_use]
    pub fn with_context(mut self, context: impl IntoIterator<Item = TranslationContext>) -> Self {
        self.context = context.into_iter().collect();
        self
    }
}

/// One earlier source/translation pair used for terminology and dialogue continuity.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TranslationContext {
    pub source: String,
    pub translation: String,
}

impl TranslationContext {
    #[must_use]
    pub fn new(source: impl Into<String>, translation: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            translation: translation.into(),
        }
    }
}

/// A translation whose entries correspond one-for-one with the request segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    pub segments: Vec<String>,
}

/// A translation backend. Implementations must preserve segment count and order.
#[async_trait]
pub trait Translator: Send + Sync {
    fn provider(&self) -> &'static str;

    async fn translate(&self, request: TranslationRequest) -> Result<Translation>;
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{provider} does not support target language {language}")]
    UnsupportedLanguage {
        provider: &'static str,
        language: Language,
    },
    #[error("{provider} does not support source language {language}")]
    UnsupportedSourceLanguage {
        provider: &'static str,
        language: Language,
    },
    #[error("{provider} returned {actual} segments; expected {expected}")]
    SegmentCount {
        provider: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("{provider} quota or rate limit was exceeded")]
    QuotaExceeded { provider: &'static str },
    #[error("{provider} API request failed with HTTP {status}: {message}")]
    Api {
        provider: &'static str,
        status: u16,
        message: String,
    },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
