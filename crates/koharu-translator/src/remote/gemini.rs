// Ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/gemini.rs

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

use super::send_json;
use crate::{RemoteProviderKind, Result, TranslationRequest, prompt};

const ROOT: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct GeminiConfig {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: bool,
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            model: "gemini-2.5-flash".into(),
            temperature: None,
            max_tokens: None,
            thinking: false,
        }
    }
}

impl GeminiConfig {
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
    config: &GeminiConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let provider = RemoteProviderKind::Gemini;
    let api_key = koharu_secrets::get(provider.id())?
        .filter(|value| !value.expose_secret().trim().is_empty())
        .with_context(|| format!("{} API key is not configured", provider.id()))?;
    let (system, user) = prompt::prompts(request)?;
    let schema = prompt::output_schema(request.segments.len());
    let mut url = Url::parse(&format!("{ROOT}/{}:generateContent", config.model))
        .expect("Gemini API root is valid");
    url.query_pairs_mut()
        .append_pair("key", api_key.expose_secret());
    let body = Request {
        system_instruction: Content::new(&system),
        contents: [Content::new(&user)],
        generation_config: GenerationConfig {
            temperature: config.temperature,
            max_output_tokens: config.max_tokens,
            thinking_config: config.model.starts_with("gemini-2.5-flash").then_some(
                ThinkingConfig {
                    thinking_budget: if config.thinking { -1 } else { 0 },
                },
            ),
            response_mime_type: "application/json",
            response_json_schema: schema,
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
    Ok(prompt::translations("gemini", &text, &request.segments)?)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_config: Option<ThinkingConfig>,
    response_mime_type: &'static str,
    response_json_schema: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThinkingConfig {
    thinking_budget: i32,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_structured_output_configuration() {
        let config = GenerationConfig {
            temperature: None,
            max_output_tokens: None,
            thinking_config: Some(ThinkingConfig { thinking_budget: 0 }),
            response_mime_type: "application/json",
            response_json_schema: prompt::output_schema(2),
        };
        let value = serde_json::to_value(config).unwrap();
        assert_eq!(value["responseMimeType"], "application/json");
        assert_eq!(value["thinkingConfig"]["thinkingBudget"], 0);
        assert_eq!(
            value["responseJsonSchema"]["properties"]["translations"]["items"]["properties"]["id"]
                ["maximum"],
            1
        );
    }
}
