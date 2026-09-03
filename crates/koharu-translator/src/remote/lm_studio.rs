// https://lmstudio.ai/docs/developer/rest/chat
// https://lmstudio.ai/docs/developer/rest/list
// https://lmstudio.ai/docs/developer/openai-compat/structured-output

use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::Deserialize;
use url::Url;

use super::{
    openai_compatible::{ChatBackend, ResponseMode},
    send_json,
};
use crate::{GenerationConfig, Model, Provider, Result, TranslationRequest, display_name};

const DEFAULT_BASE_URL: &str = "http://localhost:1234";

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct LmStudioConfig {
    pub base_url: Option<Url>,
}

impl Default for LmStudioConfig {
    fn default() -> Self {
        Self {
            base_url: Some(Url::parse(DEFAULT_BASE_URL).expect("default LM Studio URL is valid")),
        }
    }
}

pub(super) async fn translate(
    client: &Client,
    config: &LmStudioConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get("lm-studio")?;
    let endpoint = endpoint(config.base_url.as_ref(), "v1/chat/completions");
    let backend = ChatBackend {
        reasoning_effort: generation
            .reasoning
            .map(|enabled| if enabled { "medium" } else { "none" }),
        ..ChatBackend::new(
            "lm-studio",
            &endpoint,
            api_key.as_ref().map(ExposeSecret::expose_secret),
            model,
            generation,
            ResponseMode::JsonSchema,
        )
    };
    super::openai_compatible::translate(client, backend, request).await
}

pub(super) async fn models(client: &Client, config: &LmStudioConfig) -> Result<Vec<Model>> {
    let api_key = koharu_secrets::get("lm-studio")?;
    let request = client.get(endpoint(config.base_url.as_ref(), "api/v1/models"));
    let request = match api_key {
        Some(api_key) => request.bearer_auth(api_key.expose_secret()),
        None => request,
    };
    let response: ModelsResponse = send_json("lm-studio", request).await?;
    Ok(response
        .models
        .into_iter()
        .filter(|model| matches!(model.kind.as_str(), "llm" | "vlm"))
        .map(|model| {
            let capabilities = model.capabilities;
            Model {
                provider: Provider::LmStudio,
                name: display_name(&model.key),
                model: Some(model.key),
                quantizations: Vec::new(),
                vision: capabilities
                    .as_ref()
                    .is_some_and(|capabilities| capabilities.vision),
                reasoning: capabilities
                    .is_some_and(|capabilities| capabilities.reasoning.is_some()),
            }
        })
        .collect())
}

fn endpoint(base_url: Option<&Url>, suffix: &str) -> String {
    let base_url = base_url.map_or(DEFAULT_BASE_URL, Url::as_str);
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

#[derive(Deserialize)]
struct ModelsResponse {
    models: Vec<ListedModel>,
}

#[derive(Deserialize)]
struct ListedModel {
    #[serde(rename = "type")]
    kind: String,
    key: String,
    capabilities: Option<ListedModelCapabilities>,
}

#[derive(Deserialize)]
struct ListedModelCapabilities {
    vision: bool,
    reasoning: Option<ListedModelReasoning>,
}

#[derive(Deserialize)]
struct ListedModelReasoning {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_structured_chat_and_native_model_endpoints() {
        let base_url = Url::parse("http://localhost:1234/").unwrap();
        assert_eq!(
            endpoint(Some(&base_url), "v1/chat/completions"),
            "http://localhost:1234/v1/chat/completions"
        );
        assert_eq!(
            endpoint(Some(&base_url), "api/v1/models"),
            "http://localhost:1234/api/v1/models"
        );
    }

    #[test]
    fn model_discovery_keeps_language_and_vision_models() {
        let response: ModelsResponse = serde_json::from_value(serde_json::json!({
            "models": [
                {
                    "type": "llm",
                    "key": "publisher/chat-model",
                    "capabilities": { "vision": false }
                },
                {
                    "type": "llm",
                    "key": "publisher/vision-model",
                    "capabilities": {
                        "vision": true,
                        "reasoning": {
                            "allowed_options": ["off", "on"],
                            "default": "on"
                        }
                    }
                },
                {
                    "type": "embedding",
                    "key": "publisher/embed-model"
                }
            ]
        }))
        .unwrap();
        let models = response
            .models
            .into_iter()
            .filter(|model| matches!(model.kind.as_str(), "llm" | "vlm"))
            .map(|model| {
                let capabilities = model.capabilities;
                (
                    model.key,
                    capabilities
                        .as_ref()
                        .is_some_and(|capabilities| capabilities.vision),
                    capabilities.is_some_and(|capabilities| capabilities.reasoning.is_some()),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            models,
            [
                ("publisher/chat-model".to_owned(), false, false),
                ("publisher/vision-model".to_owned(), true, true)
            ]
        );
    }
}
