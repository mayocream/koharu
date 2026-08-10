// Ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/claude.rs

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::send_json;
use crate::{GenerationConfig, Model, Provider, Result, TranslationRequest, prompt};

const URL: &str = "https://api.anthropic.com/v1/messages";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct ClaudeConfig {}

pub(super) static MODELS: &[(&str, &str)] = &[
    ("claude-fable-5", "Claude Fable 5"),
    ("claude-opus-4-8", "Claude Opus 4.8"),
    ("claude-sonnet-5", "Claude Sonnet 5"),
    ("claude-haiku-4-5", "Claude Haiku 4.5"),
    ("claude-opus-4-7", "Claude Opus 4.7"),
    ("claude-sonnet-4-6", "Claude Sonnet 4.6"),
    ("claude-opus-4-6", "Claude Opus 4.6"),
    ("claude-opus-4-5-20251101", "Claude Opus 4.5"),
    ("claude-haiku-4-5-20251001", "Claude Haiku 4.5 Snapshot"),
];

pub(super) async fn models() -> Result<Vec<Model>> {
    Ok(if koharu_secrets::get("claude")?.is_some() {
        Model::catalog(Provider::Claude, MODELS)
    } else {
        Vec::new()
    })
}

pub(super) async fn translate(
    client: &Client,
    _config: &ClaudeConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get("claude")?.context("claude API key is not configured")?;
    let (system, user) = prompt::prompts(request)?;
    let body = Request {
        model,
        max_tokens: generation.max_tokens.unwrap_or(8192),
        system: &system,
        messages: [Message {
            role: "user",
            content: &user,
        }],
        temperature: generation.temperature,
        thinking: model
            .starts_with("claude-sonnet-5")
            .then_some(ThinkingConfig {
                kind: if generation.thinking {
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
