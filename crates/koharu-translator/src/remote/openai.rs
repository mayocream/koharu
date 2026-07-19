use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::openai_compatible::{ChatBackend, ResponseMode};
use crate::{RemoteProviderKind, Result, TranslationRequest};

const URL: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct OpenAiConfig {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: bool,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            model: "gpt-4.1-mini".into(),
            temperature: None,
            max_tokens: None,
            thinking: false,
        }
    }
}

impl OpenAiConfig {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            temperature: None,
            max_tokens: None,
            thinking: false,
        }
    }
}

pub(super) async fn translate(
    client: &Client,
    config: &OpenAiConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let provider = RemoteProviderKind::OpenAi;
    let api_key = koharu_secrets::get(provider.id())?
        .filter(|value| !value.expose_secret().trim().is_empty())
        .with_context(|| format!("{} API key is not configured", provider.id()))?;
    super::openai_compatible::translate(
        client,
        ChatBackend {
            provider: "openai",
            endpoint: URL,
            api_key: Some(api_key.expose_secret()),
            model: &config.model,
            temperature: config.temperature,
            max_tokens: None,
            max_completion_tokens: config.max_tokens,
            reasoning_effort: ["gpt-5.1", "gpt-5.2", "gpt-5.4", "gpt-5.5", "gpt-5.6"]
                .iter()
                .any(|prefix| config.model.starts_with(prefix))
                .then_some(if config.thinking { "medium" } else { "none" }),
            reasoning: None,
            thinking: None,
            response_mode: ResponseMode::JsonSchema,
        },
        request,
    )
    .await
}
