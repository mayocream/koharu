// Request shape aligned with:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/chat_completions.rs

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

use super::send_json;
use crate::{RemoteProviderKind, Result, TranslationRequest, prompt};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct OpenAiCompatibleConfig {
    pub base_url: Url,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl OpenAiCompatibleConfig {
    #[must_use]
    pub fn new(base_url: Url, model: impl Into<String>) -> Self {
        Self {
            base_url,
            model: model.into(),
            temperature: None,
            max_tokens: None,
        }
    }
}

pub(super) async fn compatible(
    client: &Client,
    config: &OpenAiCompatibleConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get(RemoteProviderKind::OpenAiCompatible.id())?
        .filter(|value| !value.expose_secret().trim().is_empty());
    let endpoint = endpoint(&config.base_url, "chat/completions");
    translate(
        client,
        ChatBackend {
            provider: "openai-compatible",
            endpoint: &endpoint,
            api_key: api_key.as_ref().map(ExposeSecret::expose_secret),
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

pub(super) async fn translate(
    client: &Client,
    backend: ChatBackend<'_>,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let (system, user) = prompt::prompts(request)?;
    let body = ChatRequest {
        model: backend.model,
        messages: [
            Message {
                role: "system",
                content: &system,
            },
            Message {
                role: "user",
                content: &user,
            },
        ],
        temperature: backend.temperature,
        max_tokens: backend.max_tokens,
        max_completion_tokens: backend.max_completion_tokens,
        reasoning_effort: backend.reasoning_effort,
        reasoning: backend.reasoning.map(|enabled| ReasoningConfig { enabled }),
        thinking: backend.thinking.map(|kind| ThinkingConfig { kind }),
        response_format: backend
            .response_mode
            .response_format(request.segments.len()),
    };
    let mut builder = client.post(backend.endpoint).json(&body);
    if let Some(api_key) = backend.api_key {
        builder = builder.bearer_auth(api_key);
    }
    let response: ChatResponse = send_json(backend.provider, builder).await?;
    let text = response
        .choices
        .into_iter()
        .next()
        .context("chat completion returned no choices")?
        .message
        .content;
    Ok(prompt::translations(
        backend.provider,
        &text,
        &request.segments,
    )?)
}

pub(super) struct ChatBackend<'a> {
    pub(super) provider: &'static str,
    pub(super) endpoint: &'a str,
    pub(super) api_key: Option<&'a str>,
    pub(super) model: &'a str,
    pub(super) temperature: Option<f32>,
    pub(super) max_tokens: Option<u32>,
    pub(super) max_completion_tokens: Option<u32>,
    pub(super) reasoning_effort: Option<&'static str>,
    pub(super) reasoning: Option<bool>,
    pub(super) thinking: Option<&'static str>,
    pub(super) response_mode: ResponseMode,
}

/// Lists models exposed by an OpenAI-compatible `/models` endpoint.
pub async fn discover_openai_compatible_models(
    client: &Client,
    base_url: &Url,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get(RemoteProviderKind::OpenAiCompatible.id())?
        .filter(|value| !value.expose_secret().trim().is_empty());
    let mut builder = client.get(endpoint(base_url, "models"));
    if let Some(api_key) = api_key {
        builder = builder.bearer_auth(api_key.expose_secret());
    }
    discover_models("openai-compatible", builder).await
}

pub(super) async fn discover_models(
    provider: &'static str,
    request: reqwest::RequestBuilder,
) -> Result<Vec<String>> {
    let response: ModelsResponse = send_json(provider, request).await?;
    Ok(response.data.into_iter().map(|model| model.id).collect())
}

fn endpoint(base_url: &Url, suffix: &str) -> String {
    format!(
        "{}/{}",
        base_url.as_str().trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [Message<'a>; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Clone, Copy, Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Clone, Copy, Serialize)]
struct ReasoningConfig {
    enabled: bool,
}

#[derive(Clone, Copy)]
pub(super) enum ResponseMode {
    PromptOnly,
    JsonObject,
    JsonSchema,
}

impl ResponseMode {
    fn response_format(self, expected: usize) -> Option<ResponseFormat> {
        match self {
            Self::PromptOnly => None,
            Self::JsonObject => Some(ResponseFormat {
                kind: "json_object",
                json_schema: None,
            }),
            Self::JsonSchema => Some(ResponseFormat {
                kind: "json_schema",
                json_schema: Some(JsonSchema {
                    name: "manga_translation",
                    strict: true,
                    schema: prompt::output_schema(expected),
                }),
            }),
        }
    }
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_schema: Option<JsonSchema>,
}

#[derive(Serialize)]
struct JsonSchema {
    name: &'static str,
    strict: bool,
    schema: serde_json::Value,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_preserves_base_path() {
        let url = Url::parse("http://localhost:1234/v1").unwrap();
        assert_eq!(endpoint(&url, "models"), "http://localhost:1234/v1/models");
    }

    #[test]
    fn serializes_provider_specific_response_formats() {
        assert_eq!(
            serde_json::to_value(ResponseMode::JsonObject.response_format(2).unwrap()).unwrap(),
            serde_json::json!({ "type": "json_object" })
        );

        let strict =
            serde_json::to_value(ResponseMode::JsonSchema.response_format(2).unwrap()).unwrap();
        assert_eq!(strict["type"], "json_schema");
        assert_eq!(strict["json_schema"]["name"], "manga_translation");
        assert_eq!(strict["json_schema"]["strict"], true);
        assert_eq!(
            strict["json_schema"]["schema"]["properties"]["translations"]["items"]["properties"]["id"]
                ["maximum"],
            1
        );
        assert!(ResponseMode::PromptOnly.response_format(2).is_none());
    }

    #[test]
    fn serializes_current_completion_fields() {
        let request = ChatRequest {
            model: "gpt-5.6-luna",
            messages: [
                Message {
                    role: "system",
                    content: "system",
                },
                Message {
                    role: "user",
                    content: "user",
                },
            ],
            temperature: None,
            max_tokens: None,
            max_completion_tokens: Some(1024),
            reasoning_effort: Some("none"),
            reasoning: None,
            thinking: None,
            response_format: None,
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["max_completion_tokens"], 1024);
        assert_eq!(value["reasoning_effort"], "none");
        assert!(value.get("max_tokens").is_none());
        assert!(value.get("thinking").is_none());
    }

    #[test]
    fn serializes_openrouter_reasoning_control() {
        assert_eq!(
            serde_json::to_value(ReasoningConfig { enabled: false }).unwrap(),
            serde_json::json!({ "enabled": false })
        );
    }
}
