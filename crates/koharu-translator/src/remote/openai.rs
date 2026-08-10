use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;

use super::openai_compatible::{ChatBackend, ResponseMode};
use crate::{GenerationConfig, Model, Provider, Result, TranslationRequest};

const URL: &str = "https://api.openai.com/v1/chat/completions";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct OpenAiConfig {}

pub(super) static MODELS: &[(&str, &str)] = &[
    ("gpt-5.6", "5.6"),
    ("gpt-5.6-sol", "5.6 Sol"),
    ("gpt-5.6-terra", "5.6 Terra"),
    ("gpt-5.6-luna", "5.6 Luna"),
    ("gpt-5.5", "5.5"),
    ("gpt-5.4", "5.4"),
    ("gpt-5.4-mini", "5.4 mini"),
    ("gpt-5.4-nano", "5.4 nano"),
    ("gpt-5.2", "5.2"),
    ("gpt-5.1", "5.1"),
    ("gpt-5", "5"),
    ("gpt-5-mini", "5 mini"),
    ("gpt-5-nano", "5 nano"),
    ("o3", "o3"),
    ("gpt-4.1", "4.1"),
    ("gpt-4.1-mini", "4.1 mini"),
    ("gpt-4o-mini", "4o mini"),
];

pub(super) async fn models() -> Result<Vec<Model>> {
    Ok(if koharu_secrets::get("openai")?.is_some() {
        Model::catalog(Provider::OpenAi, MODELS, true)
    } else {
        Vec::new()
    })
}

pub(super) async fn translate(
    client: &Client,
    _config: &OpenAiConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get("openai")?.context("openai API key is not configured")?;
    let mut backend = ChatBackend::new(
        "openai",
        URL,
        Some(api_key.expose_secret()),
        model,
        generation,
        ResponseMode::JsonSchema,
    );
    backend.max_tokens = None;
    backend.max_completion_tokens = generation.max_tokens;
    backend.reasoning_effort = ["gpt-5.1", "gpt-5.2", "gpt-5.4", "gpt-5.5", "gpt-5.6"]
        .iter()
        .any(|prefix| model.starts_with(prefix))
        .then_some(if generation.thinking {
            "medium"
        } else {
            "none"
        });
    super::openai_compatible::translate(client, backend, request).await
}
