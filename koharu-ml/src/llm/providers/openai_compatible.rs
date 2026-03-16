use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use koharu_http::http::http_client;

use crate::llm::prompt::system_prompt;

use super::{AnyProvider, ensure_provider_success};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    pub base_url: String,
    pub api_key: Option<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
}

fn normalized_base_url(base_url: &str) -> anyhow::Result<String> {
    let normalized = base_url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        anyhow::bail!("OpenAI-compatible base URL is required");
    }
    Ok(normalized)
}

pub async fn list_models(base_url: &str, api_key: Option<&str>) -> anyhow::Result<Vec<String>> {
    let endpoint = format!("{}/models", normalized_base_url(base_url)?);
    let mut request = http_client().get(endpoint);

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
        target_language: &'a str,
        model: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let body = ChatRequest {
                model,
                messages: vec![
                    ChatMessage {
                        role: "system",
                        content: system_prompt(target_language),
                    },
                    ChatMessage {
                        role: "user",
                        content: source.to_string(),
                    },
                ],
            };

            let endpoint = format!("{}/chat/completions", normalized_base_url(&self.base_url)?);
            let mut request = http_client().post(endpoint);
            if let Some(api_key) = self
                .api_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                request = request.bearer_auth(api_key);
            }

            let response = request
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&body)?)
                .send()
                .await?;

            let resp: serde_json::Value = ensure_provider_success("openai-compatible", response)
                .await?
                .json()
                .await?;

            let text = resp["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("OpenAI-compatible provider returned no content"))?
                .to_string();

            Ok(text)
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
