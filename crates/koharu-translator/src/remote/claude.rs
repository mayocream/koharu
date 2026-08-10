// Ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/claude.rs

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::send_json;
use crate::{
    GenerationConfig, Model, Provider, Result, TranslationRequest, backend::encode_image, prompt,
};

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
        Model::catalog(Provider::Claude, MODELS, true)
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
        messages: [Message::user(&user, request.image.as_deref())?],
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
    content: Vec<ContentBlock<'a>>,
}

impl<'a> Message<'a> {
    fn user(text: &'a str, image: Option<&image::DynamicImage>) -> anyhow::Result<Self> {
        let mut content = vec![ContentBlock::Text { text }];
        if let Some(image) = image {
            content.push(ContentBlock::Image {
                source: ImageSource {
                    kind: "base64",
                    media_type: "image/jpeg",
                    data: encode_image(image)?.data,
                },
            });
        }
        Ok(Self {
            role: "user",
            content,
        })
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock<'a> {
    Text { text: &'a str },
    Image { source: ImageSource },
}

#[derive(Serialize)]
struct ImageSource {
    #[serde(rename = "type")]
    kind: &'static str,
    media_type: &'static str,
    data: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_text_and_base64_image_blocks() {
        let message =
            Message::user("translate", Some(&image::DynamicImage::new_rgb8(1, 1))).unwrap();
        let value = serde_json::to_value(message).unwrap();
        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][1]["type"], "image");
        assert_eq!(value["content"][1]["source"]["type"], "base64");
        assert_eq!(value["content"][1]["source"]["media_type"], "image/jpeg");
    }
}
