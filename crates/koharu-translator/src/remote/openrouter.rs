// https://openrouter.ai/docs/api/reference/overview
// https://openrouter.ai/docs/guides/best-practices/reasoning-tokens

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::openai_compatible::{ChatBackend, ResponseMode};
use crate::{RemoteProviderKind, Result, TranslationRequest};

const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct OpenRouterConfig {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: bool,
}

impl Default for OpenRouterConfig {
    fn default() -> Self {
        Self {
            model: "openrouter/auto".into(),
            temperature: None,
            max_tokens: None,
            thinking: false,
        }
    }
}

impl OpenRouterConfig {
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
    config: &OpenRouterConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let provider = RemoteProviderKind::OpenRouter;
    let api_key = koharu_secrets::get(provider.id())?
        .filter(|value| !value.expose_secret().trim().is_empty())
        .with_context(|| format!("{} API key is not configured", provider.id()))?;
    super::openai_compatible::translate(
        client,
        ChatBackend {
            provider: "openrouter",
            endpoint: CHAT_URL,
            api_key: Some(api_key.expose_secret()),
            model: &config.model,
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            max_completion_tokens: None,
            reasoning_effort: None,
            reasoning: Some(config.thinking),
            thinking: None,
            response_mode: ResponseMode::PromptOnly,
        },
        request,
    )
    .await
}

/// Lists text-output models exposed by OpenRouter.
pub async fn discover_openrouter_models(client: &Client) -> Result<Vec<String>> {
    let provider = RemoteProviderKind::OpenRouter;
    let api_key = koharu_secrets::get(provider.id())?
        .filter(|value| !value.expose_secret().trim().is_empty())
        .with_context(|| format!("{} API key is not configured", provider.id()))?;
    super::openai_compatible::discover_models(
        "openrouter",
        client.get(MODELS_URL).bearer_auth(api_key.expose_secret()),
    )
    .await
}
