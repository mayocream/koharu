// https://openrouter.ai/docs/api/reference/overview
// https://openrouter.ai/docs/guides/best-practices/reasoning-tokens

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::Deserialize;

use super::openai_compatible::{ChatBackend, ResponseMode};
use crate::{GenerationConfig, Model, Provider, Result, TranslationRequest};

const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct OpenRouterConfig {}

pub(super) static MODELS: &[(&str, &str)] = &[("openrouter/auto", "OpenRouter Auto")];

pub(super) async fn translate(
    client: &Client,
    _config: &OpenRouterConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key =
        koharu_secrets::get("openrouter")?.context("openrouter API key is not configured")?;
    let mut backend = ChatBackend::new(
        "openrouter",
        CHAT_URL,
        Some(api_key.expose_secret()),
        model,
        generation,
        ResponseMode::PromptOnly,
    );
    backend.reasoning = Some(generation.thinking);
    super::openai_compatible::translate(client, backend, request).await
}

pub(super) async fn models(client: &Client) -> Result<Vec<Model>> {
    let Some(api_key) = koharu_secrets::get("openrouter")? else {
        return Ok(Vec::new());
    };
    let mut models = Model::catalog(Provider::OpenRouter, MODELS, true);
    let discovered: Result<ModelsResponse> = super::send_json(
        "openrouter",
        client.get(MODELS_URL).bearer_auth(api_key.expose_secret()),
    )
    .await;
    match discovered {
        Ok(discovered) => models.extend(discovered.data.into_iter().map(|model| {
            Model {
                provider: Provider::OpenRouter,
                name: model.name,
                model: Some(model.id),
                quantizations: Vec::new(),
                vision: model
                    .architecture
                    .input_modalities
                    .iter()
                    .any(|modality| modality == "image"),
            }
        })),
        Err(error) => tracing::warn!(%error, "failed to list OpenRouter models"),
    }
    Ok(models)
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ListedModel>,
}

#[derive(Deserialize)]
struct ListedModel {
    id: String,
    name: String,
    architecture: Architecture,
}

#[derive(Deserialize)]
struct Architecture {
    input_modalities: Vec<String>,
}
