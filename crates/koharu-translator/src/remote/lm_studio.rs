// https://lmstudio.ai/docs/developer/rest/chat
// https://lmstudio.ai/docs/developer/rest/list

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

use super::send_json;
use crate::{RemoteProviderKind, Result, TranslationRequest, prompt};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct LmStudioConfig {
    pub base_url: Url,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: bool,
}

impl Default for LmStudioConfig {
    fn default() -> Self {
        Self {
            base_url: Url::parse("http://localhost:1234").expect("default LM Studio URL is valid"),
            model: "model".into(),
            temperature: None,
            max_tokens: None,
            thinking: false,
        }
    }
}

impl LmStudioConfig {
    #[must_use]
    pub fn new(base_url: Url, model: impl Into<String>) -> Self {
        Self {
            base_url,
            model: model.into(),
            temperature: None,
            max_tokens: None,
            thinking: false,
        }
    }
}

pub(super) async fn translate(
    client: &Client,
    config: &LmStudioConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let (system, input) = prompt::prompts(request)?;
    let body = ChatRequest {
        model: &config.model,
        input: &input,
        system_prompt: &system,
        temperature: config.temperature,
        max_output_tokens: config.max_tokens,
        reasoning: if config.thinking { "on" } else { "off" },
        store: false,
    };
    let response: ChatResponse = send_json(
        "lm-studio",
        authenticate(client.post(endpoint(&config.base_url, "chat")).json(&body))?,
    )
    .await?;
    let text = response
        .output
        .into_iter()
        .rev()
        .find_map(|output| {
            (output.kind == "message")
                .then_some(output.content)
                .flatten()
        })
        .context("LM Studio returned no message output")?;
    Ok(prompt::translations("lm-studio", &text, &request.segments)?)
}

/// Lists locally available LLMs exposed by LM Studio's native API.
pub async fn discover_lm_studio_models(client: &Client, base_url: &Url) -> Result<Vec<String>> {
    let response: ModelsResponse = send_json(
        "lm-studio",
        authenticate(client.get(endpoint(base_url, "models")))?,
    )
    .await?;
    Ok(response
        .models
        .into_iter()
        .filter_map(|model| (model.kind == "llm").then_some(model.key))
        .collect())
}

fn authenticate(mut request: RequestBuilder) -> Result<RequestBuilder> {
    let api_key = koharu_secrets::get(RemoteProviderKind::LmStudio.id())?
        .filter(|value| !value.expose_secret().trim().is_empty());
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key.expose_secret());
    }
    Ok(request)
}

fn endpoint(base_url: &Url, suffix: &str) -> String {
    format!(
        "{}/api/v1/{}",
        base_url.as_str().trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    input: &'a str,
    system_prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    reasoning: &'static str,
    store: bool,
}

#[derive(Deserialize)]
struct ChatResponse {
    output: Vec<Output>,
}

#[derive(Deserialize)]
struct Output {
    #[serde(rename = "type")]
    kind: String,
    content: Option<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    models: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    #[serde(rename = "type")]
    kind: String,
    key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_native_v1_endpoints() {
        let base_url = Url::parse("http://localhost:1234/").unwrap();
        assert_eq!(
            endpoint(&base_url, "chat"),
            "http://localhost:1234/api/v1/chat"
        );
        assert_eq!(
            endpoint(&base_url, "models"),
            "http://localhost:1234/api/v1/models"
        );
    }

    #[test]
    fn serializes_native_chat_options() {
        let value = serde_json::to_value(ChatRequest {
            model: "publisher/model",
            input: "input",
            system_prompt: "system",
            temperature: Some(0.2),
            max_output_tokens: Some(1024),
            reasoning: "off",
            store: false,
        })
        .unwrap();
        assert_eq!(value["max_output_tokens"], 1024);
        assert_eq!(value["reasoning"], "off");
        assert_eq!(value["store"], false);
        assert!(value.get("messages").is_none());
    }

    #[test]
    fn model_discovery_can_filter_non_llms() {
        let response: ModelsResponse = serde_json::from_value(serde_json::json!({
            "models": [
                { "type": "llm", "key": "publisher/chat-model" },
                { "type": "embedding", "key": "publisher/embed-model" }
            ]
        }))
        .unwrap();
        let models = response
            .models
            .into_iter()
            .filter_map(|model| (model.kind == "llm").then_some(model.key))
            .collect::<Vec<_>>();
        assert_eq!(models, ["publisher/chat-model"]);
    }
}
