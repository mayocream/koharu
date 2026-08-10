use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;

use super::openai_compatible::{ChatBackend, ResponseMode};
use crate::{GenerationConfig, Model, Provider, Result, TranslationRequest};

const URL: &str = "https://api.deepseek.com/chat/completions";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct DeepSeekConfig {}

pub(super) static MODELS: &[(&str, &str)] = &[
    ("deepseek-v4-flash", "DeepSeek V4 Flash"),
    ("deepseek-v4-pro", "DeepSeek V4 Pro"),
];

pub(super) async fn models() -> Result<Vec<Model>> {
    Ok(if koharu_secrets::get("deepseek")?.is_some() {
        Model::catalog(Provider::DeepSeek, MODELS, false)
    } else {
        Vec::new()
    })
}

pub(super) async fn translate(
    client: &Client,
    _config: &DeepSeekConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get("deepseek")?.context("deepseek API key is not configured")?;
    let mut backend = ChatBackend::new(
        "deepseek",
        URL,
        Some(api_key.expose_secret()),
        model,
        generation,
        ResponseMode::JsonObject,
    );
    backend.temperature = generation.temperature.or(Some(1.3));
    backend.thinking = Some(if generation.thinking {
        "enabled"
    } else {
        "disabled"
    });
    super::openai_compatible::translate(client, backend, request).await
}
