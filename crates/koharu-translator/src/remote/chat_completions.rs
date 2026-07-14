// Request shape aligned with:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/chat_completions.rs

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use super::{ApiKey, RemoteGenerationOptions, chat, send_json};
use crate::{Result, TranslationRequest};

const OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEEPSEEK_URL: &str = "https://api.deepseek.com/chat/completions";

macro_rules! config {
    ($name:ident) => {
        #[derive(Debug, Clone)]
        pub struct $name {
            pub api_key: ApiKey,
            pub model: String,
        }

        impl $name {
            #[must_use]
            pub fn new(api_key: impl Into<ApiKey>, model: impl Into<String>) -> Self {
                Self {
                    api_key: api_key.into(),
                    model: model.into(),
                }
            }
        }
    };
}

config!(OpenAiConfig);
config!(DeepSeekConfig);

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    pub base_url: Url,
    pub api_key: Option<ApiKey>,
    pub model: String,
}

impl OpenAiCompatibleConfig {
    #[must_use]
    pub fn new(base_url: Url, model: impl Into<String>) -> Self {
        Self {
            base_url,
            api_key: None,
            model: model.into(),
        }
    }

    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<ApiKey>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }
}

pub(super) async fn openai(
    client: &Client,
    config: &OpenAiConfig,
    generation: RemoteGenerationOptions,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    translate(
        client,
        "openai",
        OPENAI_URL,
        Some(&config.api_key),
        &config.model,
        generation,
        request,
    )
    .await
}

pub(super) async fn deepseek(
    client: &Client,
    config: &DeepSeekConfig,
    mut generation: RemoteGenerationOptions,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    generation.temperature.get_or_insert(1.3);
    translate(
        client,
        "deepseek",
        DEEPSEEK_URL,
        Some(&config.api_key),
        &config.model,
        generation,
        request,
    )
    .await
}

pub(super) async fn compatible(
    client: &Client,
    config: &OpenAiCompatibleConfig,
    generation: RemoteGenerationOptions,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    translate(
        client,
        "openai-compatible",
        &endpoint(&config.base_url, "chat/completions"),
        config.api_key.as_ref(),
        &config.model,
        generation,
        request,
    )
    .await
}

async fn translate(
    client: &Client,
    provider: &'static str,
    endpoint: &str,
    api_key: Option<&ApiKey>,
    model: &str,
    generation: RemoteGenerationOptions,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let (system, user) = chat::prompts(request)?;
    let body = ChatRequest {
        model,
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
        temperature: generation.temperature,
        max_tokens: generation.max_tokens,
    };
    let mut builder = client.post(endpoint).json(&body);
    if let Some(api_key) = api_key {
        builder = builder.bearer_auth(api_key.expose());
    }
    let response: ChatResponse = send_json(provider, builder).await?;
    let text = response
        .choices
        .into_iter()
        .next()
        .context("chat completion returned no choices")?
        .message
        .content;
    Ok(chat::translations(provider, &text)?)
}

/// Lists models exposed by an OpenAI-compatible `/models` endpoint.
pub async fn discover_openai_compatible_models(
    client: &Client,
    base_url: &Url,
    api_key: Option<&ApiKey>,
) -> Result<Vec<String>> {
    let mut builder = client.get(endpoint(base_url, "models"));
    if let Some(api_key) = api_key {
        builder = builder.bearer_auth(api_key.expose());
    }
    let response: ModelsResponse = send_json("openai-compatible", builder).await?;
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
}
