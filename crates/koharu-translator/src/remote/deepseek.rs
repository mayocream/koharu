use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::openai_compatible::{ChatBackend, ResponseMode};
use crate::{RemoteProviderKind, Result, TranslationRequest};

const URL: &str = "https://api.deepseek.com/chat/completions";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct DeepSeekConfig {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: bool,
}

impl Default for DeepSeekConfig {
    fn default() -> Self {
        Self {
            model: "deepseek-v4-flash".into(),
            temperature: None,
            max_tokens: None,
            thinking: false,
        }
    }
}

impl DeepSeekConfig {
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
    config: &DeepSeekConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let provider = RemoteProviderKind::DeepSeek;
    let api_key = koharu_secrets::get(provider.id())?
        .filter(|value| !value.expose_secret().trim().is_empty())
        .with_context(|| format!("{} API key is not configured", provider.id()))?;
    super::openai_compatible::translate(
        client,
        ChatBackend {
            provider: "deepseek",
            endpoint: URL,
            api_key: Some(api_key.expose_secret()),
            model: &config.model,
            temperature: config.temperature.or(Some(1.3)),
            max_tokens: config.max_tokens,
            max_completion_tokens: None,
            reasoning_effort: None,
            reasoning: None,
            thinking: Some(if config.thinking {
                "enabled"
            } else {
                "disabled"
            }),
            response_mode: ResponseMode::JsonObject,
        },
        request,
    )
    .await
}
