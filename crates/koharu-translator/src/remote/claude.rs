// Ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/claude.rs

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{ApiKey, RemoteGenerationOptions, chat, send_json};
use crate::{Result, TranslationRequest};

const URL: &str = "https://api.anthropic.com/v1/messages";

#[derive(Debug, Clone)]
pub struct ClaudeConfig {
    pub api_key: ApiKey,
    pub model: String,
}

impl ClaudeConfig {
    #[must_use]
    pub fn new(api_key: impl Into<ApiKey>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
        }
    }
}

pub(super) async fn translate(
    client: &Client,
    config: &ClaudeConfig,
    generation: RemoteGenerationOptions,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let (system, user) = chat::prompts(request)?;
    let body = Request {
        model: &config.model,
        max_tokens: generation.max_tokens.unwrap_or(8192),
        system: &system,
        messages: [Message {
            role: "user",
            content: &user,
        }],
        temperature: generation.temperature,
    };
    let response: Response = send_json(
        "claude",
        client
            .post(URL)
            .header("x-api-key", config.api_key.expose())
            .header("anthropic-version", "2023-06-01")
            .json(&body),
    )
    .await?;
    let text = response
        .content
        .into_iter()
        .find_map(|block| (block.kind == "text").then_some(block.text).flatten())
        .context("Claude returned no text content")?;
    Ok(chat::translations("claude", &text)?)
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: [Message<'a>; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
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
