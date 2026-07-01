use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::Language;

use super::chat_completions::{ChatCompletionsAuth, ChatCompletionsRequest, send_chat_completion};
use super::{AnyProvider, ensure_provider_success, resolve_system_prompt};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub base_url: String,
    pub api_key: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

fn normalized_base_url(base_url: &str) -> anyhow::Result<String> {
    let normalized = base_url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        anyhow::bail!("OpenAI-compatible base URL is required");
    }
    Ok(normalized)
}

pub async fn list_models(
    http_client: Arc<ClientWithMiddleware>,
    base_url: &str,
    api_key: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let endpoint = format!("{}/models", normalized_base_url(base_url)?);
    let mut request = http_client.get(endpoint);

    if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(api_key);
    }

    let response = request.send().await?;
    let models: ModelsResponse = ensure_provider_success("openai-compatible", response)
        .await?
        .json()
        .await?;

    let mut ids = models
        .data
        .into_iter()
        .map(|model| model.id)
        .filter(|id| !id.trim().is_empty())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

impl AnyProvider for OpenAiCompatibleProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        model: &'a str,
        custom_system_prompt: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let prompt = resolve_system_prompt(custom_system_prompt, target_language);
            send_chat_completion(
                Arc::clone(&self.http_client),
                ChatCompletionsRequest {
                    provider: "openai-compatible",
                    endpoint: format!("{}/chat/completions", normalized_base_url(&self.base_url)?),
                    auth: self
                        .api_key
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .map(|key| ChatCompletionsAuth::Bearer(key.to_string()))
                        .unwrap_or(ChatCompletionsAuth::None),
                    model: model.to_string(),
                    system_prompt: prompt,
                    user_prompt: source.to_string(),
                    temperature: self.temperature,
                    max_tokens: self.max_tokens,
                },
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::normalized_base_url;

    #[test]
    fn trims_trailing_slashes() {
        let normalized = normalized_base_url(" http://127.0.0.1:1234/v1/ ").unwrap();
        assert_eq!(normalized, "http://127.0.0.1:1234/v1");
    }
}
