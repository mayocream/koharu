// Ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/claude.rs

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::send_json;
use crate::{RemoteProviderKind, Result, TranslationRequest, prompt};

const URL: &str = "https://api.anthropic.com/v1/messages";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ClaudeConfig {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: bool,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-5".into(),
            temperature: None,
            max_tokens: None,
            thinking: false,
        }
    }
}

impl ClaudeConfig {
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
    config: &ClaudeConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let provider = RemoteProviderKind::Claude;
    let api_key = koharu_secrets::get(provider.id())?
        .filter(|value| !value.expose_secret().trim().is_empty())
        .with_context(|| format!("{} API key is not configured", provider.id()))?;
    let (system, user) = prompt::prompts(request)?;
    let body = Request {
        model: &config.model,
        max_tokens: config.max_tokens.unwrap_or(8192),
        system: &system,
        messages: [Message {
            role: "user",
            content: &user,
        }],
        temperature: config.temperature,
        thinking: config
            .model
            .starts_with("claude-sonnet-5")
            .then_some(ThinkingConfig {
                kind: if config.thinking {
                    "adaptive"
                } else {
                    "disabled"
                },
            }),
    };
    let response: Response = send_json(
        "claude",
        client
            .post(URL)
            .header("x-api-key", api_key.expose_secret())
            .header("anthropic-version", "2023-06-01")
            .json(&body),
    )
    .await?;
    let text = response
        .content
        .into_iter()
        .find_map(|block| (block.kind == "text").then_some(block.text).flatten())
        .context("Claude returned no text content")?;
    Ok(prompt::translations("claude", &text, &request.segments)?)
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: [Message<'a>; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
}

#[derive(Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Deserialize)]
struct Response {
    content: Vec<Content>,
}

#[derive(Deserialize)]
struct Content {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}
