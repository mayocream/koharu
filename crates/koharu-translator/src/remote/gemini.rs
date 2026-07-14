// Ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/gemini.rs

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use super::{ApiKey, RemoteGenerationOptions, chat, send_json};
use crate::{Result, TranslationRequest};

const ROOT: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Debug, Clone)]
pub struct GeminiConfig {
    pub api_key: ApiKey,
    pub model: String,
}

impl GeminiConfig {
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
    config: &GeminiConfig,
    generation: RemoteGenerationOptions,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let (system, user) = chat::prompts(request)?;
    let mut url = Url::parse(&format!("{ROOT}/{}:generateContent", config.model))
        .expect("Gemini API root is valid");
    url.query_pairs_mut()
        .append_pair("key", config.api_key.expose());
    let body = Request {
        system_instruction: Content::new(&system),
        contents: [Content::new(&user)],
        generation_config: GenerationConfig {
            temperature: generation.temperature,
            max_output_tokens: generation.max_tokens,
        },
    };
    let response: Response = send_json("gemini", client.post(url).json(&body)).await?;
    let text = response
        .candidates
        .into_iter()
        .next()
        .and_then(|candidate| candidate.content.parts.into_iter().next())
        .context("Gemini returned no candidate content")?
        .text;
    Ok(chat::translations("gemini", &text)?)
}

#[derive(Serialize)]
struct Request<'a> {
    system_instruction: Content<'a>,
    contents: [Content<'a>; 1],
    generation_config: GenerationConfig,
}

#[derive(Serialize)]
struct Content<'a> {
    parts: [Part<'a>; 1],
}

impl<'a> Content<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            parts: [Part { text }],
        }
    }
}

#[derive(Serialize)]
struct Part<'a> {
    text: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct Response {
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    content: ResponseContent,
}

#[derive(Deserialize)]
struct ResponseContent {
    parts: Vec<ResponsePart>,
}

#[derive(Deserialize)]
struct ResponsePart {
    text: String,
}
