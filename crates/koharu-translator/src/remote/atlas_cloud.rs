// https://www.atlascloud.ai/docs/models/llm
// https://www.atlascloud.ai/blog/guides/deepseek-api-atlascloud-models-pricing-quickstart

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::openai_compatible::{ChatBackend, ResponseMode};
use crate::{RemoteProviderKind, Result, TranslationRequest};

const CHAT_URL: &str = "https://api.atlascloud.ai/v1/chat/completions";
const MODELS_URL: &str = "https://api.atlascloud.ai/v1/models";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct AtlasCloudConfig {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl Default for AtlasCloudConfig {
    fn default() -> Self {
        Self {
            model: "qwen/qwen3.5-flash".into(),
            temperature: None,
            max_tokens: None,
        }
    }
}

impl AtlasCloudConfig {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            temperature: None,
            max_tokens: None,
        }
    }
}

pub(super) async fn translate(
    client: &Client,
    config: &AtlasCloudConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = api_key()?;
    super::openai_compatible::translate(
        client,
        ChatBackend {
            provider: "atlas-cloud",
            endpoint: CHAT_URL,
            api_key: Some(api_key.expose_secret()),
            model: &config.model,
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            max_completion_tokens: None,
            reasoning_effort: None,
            reasoning: None,
            thinking: None,
            response_mode: ResponseMode::PromptOnly,
        },
        request,
    )
    .await
}

/// Lists the current chat models exposed by Atlas Cloud's OpenAI-compatible catalog.
pub async fn discover_atlas_cloud_models(client: &Client) -> Result<Vec<String>> {
    let api_key = api_key()?;
    super::openai_compatible::discover_models(
        "atlas-cloud",
        client.get(MODELS_URL).bearer_auth(api_key.expose_secret()),
    )
    .await
}

fn api_key() -> Result<koharu_secrets::SecretString> {
    let provider = RemoteProviderKind::AtlasCloud;
    Ok(koharu_secrets::get(provider.id())?
        .filter(|value| !value.expose_secret().trim().is_empty())
        .with_context(|| format!("{} API key is not configured", provider.id()))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_a_current_multilingual_atlas_model() {
        assert_eq!(AtlasCloudConfig::default().model, "qwen/qwen3.5-flash");
    }

    #[test]
    fn endpoints_include_the_required_v1_prefix() {
        assert_eq!(CHAT_URL, "https://api.atlascloud.ai/v1/chat/completions");
        assert_eq!(MODELS_URL, "https://api.atlascloud.ai/v1/models");
    }
}
