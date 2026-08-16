// Ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/gemini.rs

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use super::send_json;
use crate::{
    GenerationConfig as TranslationGeneration, Model, Provider, Result, TranslationRequest,
    backend::encode_image, prompt,
};

const ROOT: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct GeminiConfig {}

pub(super) static MODELS: &[(&str, &str)] = &[
    ("gemini-flash-lite-latest", "Gemini Flash-Lite Latest"),
    ("gemini-flash-latest", "Gemini Flash Latest"),
    ("gemini-pro-latest", "Gemini Pro Latest"),
    ("gemini-3.5-flash", "Gemini 3.5 Flash"),
    ("gemini-3.1-pro-preview", "Gemini 3.1 Pro Preview"),
    (
        "gemini-3.1-pro-preview-customtools",
        "Gemini 3.1 Pro Preview Custom Tools",
    ),
    ("gemini-3.1-flash-lite", "Gemini 3.1 Flash-Lite"),
    ("gemini-3-flash-preview", "Gemini 3 Flash Preview"),
    ("gemini-2.5-pro", "Gemini 2.5 Pro"),
    ("gemini-2.5-flash", "Gemini 2.5 Flash"),
    ("gemini-2.5-flash-lite", "Gemini 2.5 Flash-Lite"),
];

pub(super) async fn models() -> Result<Vec<Model>> {
    Ok(if koharu_secrets::get("gemini")?.is_some() {
        Model::catalog(Provider::Gemini, MODELS, true)
    } else {
        Vec::new()
    })
}

pub(super) async fn translate(
    client: &Client,
    _config: &GeminiConfig,
    model: &str,
    generation: &TranslationGeneration,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get("gemini")?.context("gemini API key is not configured")?;
    let (system, user) = prompt::prompts(request)?;
    let schema = prompt::output_schema(request.segments.len());
    let mut url =
        Url::parse(&format!("{ROOT}/{model}:generateContent")).expect("Gemini API root is valid");
    url.query_pairs_mut()
        .append_pair("key", api_key.expose_secret());
    let body = Request {
        system_instruction: Content::text(&system),
        contents: [Content::user(&user, request.image.as_deref())?],
        generation_config: GenerationConfig {
            temperature: generation.temperature,
            max_output_tokens: generation.max_tokens,
            thinking_config: model
                .starts_with("gemini-2.5-flash")
                .then_some(ThinkingConfig {
                    thinking_budget: if generation.thinking { -1 } else { 0 },
                }),
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
    parts: Vec<Part<'a>>,
}

impl<'a> Content<'a> {
    fn text(text: &'a str) -> Self {
        Self {
            parts: vec![Part::Text { text }],
        }
    }

    fn user(text: &'a str, image: Option<&image::DynamicImage>) -> anyhow::Result<Self> {
        let mut parts = vec![Part::Text { text }];
        if let Some(image) = image {
            parts.push(Part::InlineData {
                inline_data: InlineData {
                    mime_type: "image/jpeg",
                    data: encode_image(image)?.data,
                },
            });
        }
        Ok(Self { parts })
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum Part<'a> {
    Text {
        text: &'a str,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: InlineData,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InlineData {
    mime_type: &'static str,
    data: String,
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
            value["responseJsonSchema"]["properties"]["1"]["type"],
            "string"
        );
        assert_eq!(
            value["responseJsonSchema"]["properties"]["2"]["type"],
            "string"
        );
        assert_eq!(
            value["responseJsonSchema"]["required"],
            serde_json::json!(["1", "2"])
        );
        assert_eq!(value["responseJsonSchema"]["additionalProperties"], false);
    }

    #[test]
    fn serializes_text_and_inline_image_parts() {
        let content =
            Content::user("translate", Some(&image::DynamicImage::new_rgb8(1, 1))).unwrap();
        let value = serde_json::to_value(content).unwrap();
        assert_eq!(value["parts"][0]["text"], "translate");
        assert_eq!(value["parts"][1]["inlineData"]["mimeType"], "image/jpeg");
        assert!(value["parts"][1]["inlineData"]["data"].is_string());
    }
}
