// https://api-docs.deepseek.com/api/list-models

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::Deserialize;

use super::openai_compatible::{ChatBackend, ResponseMode};
use super::send_json;
use crate::{GenerationConfig, Model, Provider, Result, TranslationRequest, display_name};

const URL: &str = "https://api.deepseek.com/chat/completions";
const MODELS_URL: &str = "https://api.deepseek.com/models";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct DeepSeekConfig {}

pub(super) async fn models(client: &Client) -> Result<Vec<Model>> {
    let Some(api_key) = koharu_secrets::get("deepseek")? else {
        return Ok(Vec::new());
    };
    let response: ModelsResponse = send_json(
        "deepseek",
        client.get(MODELS_URL).bearer_auth(api_key.expose_secret()),
    )
    .await?;
    Ok(response
        .data
        .into_iter()
        .map(|model| Model {
            provider: Provider::DeepSeek,
            name: display_name(&model.id),
            model: Some(model.id),
            quantizations: Vec::new(),
            vision: false,
            reasoning: true,
        })
        .collect())
}

pub(super) async fn translate(
    client: &Client,
    _config: &DeepSeekConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get("deepseek")?.context("deepseek API key is not configured")?;
    let backend = ChatBackend {
        temperature: generation.temperature.or(Some(1.3)),
        thinking: generation
            .reasoning
            .map(|enabled| if enabled { "enabled" } else { "disabled" }),
        ..ChatBackend::new(
            "deepseek",
            URL,
            Some(api_key.expose_secret()),
            model,
            generation,
            ResponseMode::JsonObject,
        )
    };
    super::openai_compatible::translate(client, backend, request).await
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ListedModel>,
}

#[derive(Deserialize)]
struct ListedModel {
    id: String,
}
