//! OpenRouter (`https://openrouter.ai`).
//!
//! OpenRouter exposes an OpenAI-compatible chat-completions API that proxies
//! hundreds of models (OpenAI, Anthropic, Google, Meta, Qwen, ...) behind a
//! single API key. Model ids use the `vendor/model` form, e.g.
//! `openai/gpt-4o-mini` or `anthropic/claude-3.5-sonnet`.
//!
//! The available models are discovered dynamically from the `/models` endpoint,
//! so the catalog always reflects what OpenRouter currently offers.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;

use crate::Language;

use super::chat_completions::{ChatCompletionsAuth, ChatCompletionsRequest, send_chat_completion};
use super::{
    AnyProvider, DiscoveredProviderModel, ensure_provider_success, resolve_system_prompt,
};

pub const BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Attribution headers OpenRouter recommends so traffic is correctly
/// associated with the app on its dashboard and public rankings.
const REFERER: &str = "https://koharu.rs";
const TITLE: &str = "Koharu";

pub struct OpenRouterProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: String,
}

fn attribution_headers() -> Vec<(&'static str, String)> {
    vec![
        ("HTTP-Referer", REFERER.to_string()),
        ("X-Title", TITLE.to_string()),
    ]
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    #[serde(default)]
    name: Option<String>,
}

/// Fetch the catalog of models OpenRouter currently serves.
pub async fn list_models(
    http_client: Arc<ClientWithMiddleware>,
    api_key: &str,
) -> anyhow::Result<Vec<DiscoveredProviderModel>> {
    let response = http_client
        .get(format!("{BASE_URL}/models"))
        .bearer_auth(api_key)
        .send()
        .await?;

    let models: ModelsResponse = ensure_provider_success("openrouter", response)
        .await?
        .json()
        .await?;

    let mut entries = models
        .data
        .into_iter()
        .filter(|model| !model.id.trim().is_empty())
        .map(|model| {
            let name = model
                .name
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| model.id.clone());
            DiscoveredProviderModel { id: model.id, name }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries.dedup_by(|a, b| a.id == b.id);
    Ok(entries)
}

impl AnyProvider for OpenRouterProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        model: &'a str,
        custom_system_prompt: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            send_chat_completion(
                Arc::clone(&self.http_client),
                ChatCompletionsRequest {
                    provider: "openrouter",
                    endpoint: format!("{BASE_URL}/chat/completions"),
                    auth: ChatCompletionsAuth::Bearer(self.api_key.clone()),
                    model: model.to_string(),
                    system_prompt: resolve_system_prompt(custom_system_prompt, target_language),
                    user_prompt: source.to_string(),
                    temperature: None,
                    max_tokens: None,
                    extra_headers: attribution_headers(),
                },
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_use_openrouter_base_url() {
        assert_eq!(BASE_URL, "https://openrouter.ai/api/v1");
        assert_eq!(
            format!("{BASE_URL}/chat/completions"),
            "https://openrouter.ai/api/v1/chat/completions"
        );
    }

    #[test]
    fn attribution_headers_are_present() {
        let headers = attribution_headers();
        assert!(headers.iter().any(|(name, _)| *name == "HTTP-Referer"));
        assert!(headers.iter().any(|(name, _)| *name == "X-Title"));
    }
}
