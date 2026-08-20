// https://openrouter.ai/docs/api/reference/overview
// https://openrouter.ai/docs/api/api-reference/models/get-models
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

pub(super) async fn translate(
    client: &Client,
    _config: &OpenRouterConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key =
        koharu_secrets::get("openrouter")?.context("openrouter API key is not configured")?;
    let backend = ChatBackend {
        reasoning: generation.reasoning,
        ..ChatBackend::new(
            "openrouter",
            CHAT_URL,
            Some(api_key.expose_secret()),
            model,
            generation,
            ResponseMode::PromptOnly,
        )
    };
    super::openai_compatible::translate(client, backend, request).await
}

pub(super) async fn models(client: &Client) -> Result<Vec<Model>> {
    let Some(api_key) = koharu_secrets::get("openrouter")? else {
        return Ok(Vec::new());
    };
    let discovered: Result<ModelsResponse> = super::send_json(
        "openrouter",
        client.get(MODELS_URL).bearer_auth(api_key.expose_secret()),
    )
    .await;
    Ok(match discovered {
        Ok(discovered) => discovered
            .data
            .into_iter()
            .map(|model| Model {
                provider: Provider::OpenRouter,
                name: model.name,
                model: Some(model.id),
                quantizations: Vec::new(),
                vision: model
                    .architecture
                    .input_modalities
                    .iter()
                    .any(|modality| modality == "image"),
                reasoning: model.reasoning.is_some()
                    || model
                        .supported_parameters
                        .iter()
                        .any(|parameter| parameter == "reasoning"),
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "failed to list OpenRouter models");
            Vec::new()
        }
    })
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
    supported_parameters: Vec<String>,
    reasoning: Option<ReasoningCapabilities>,
}

#[derive(Deserialize)]
struct ReasoningCapabilities {}

#[derive(Deserialize)]
struct Architecture {
    input_modalities: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_reasoning_capability_from_model_list() {
        let response: ModelsResponse = serde_json::from_value(serde_json::json!({
            "data": [
                {
                    "id": "provider/reasoning-model",
                    "name": "Reasoning Model",
                    "architecture": { "input_modalities": ["text"] },
                    "supported_parameters": ["reasoning"],
                    "reasoning": {
                        "supported_efforts": ["high", "medium", "low"]
                    }
                },
                {
                    "id": "provider/chat-model",
                    "name": "Chat Model",
                    "architecture": { "input_modalities": ["text"] },
                    "supported_parameters": []
                }
            ]
        }))
        .unwrap();
        assert!(response.data[0].reasoning.is_some());
        assert!(response.data[1].reasoning.is_none());
    }
}
