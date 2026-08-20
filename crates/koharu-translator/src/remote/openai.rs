// https://developers.openai.com/api/reference/resources/models/methods/list

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::Deserialize;

use super::openai_compatible::{ChatBackend, ResponseMode};
use super::send_json;
use crate::{GenerationConfig, Model, Provider, Result, TranslationRequest, display_name};

const URL: &str = "https://api.openai.com/v1/chat/completions";
const MODELS_URL: &str = "https://api.openai.com/v1/models";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct OpenAiConfig {}

pub(super) async fn models(client: &Client) -> Result<Vec<Model>> {
    let Some(api_key) = koharu_secrets::get("openai")? else {
        return Ok(Vec::new());
    };
    let response: ModelsResponse = send_json(
        "openai",
        client.get(MODELS_URL).bearer_auth(api_key.expose_secret()),
    )
    .await?;
    Ok(response
        .data
        .into_iter()
        .filter(|model| supports_translation(&model.id))
        .map(|model| Model {
            provider: Provider::OpenAi,
            name: display_name(&model.id),
            model: Some(model.id),
            quantizations: Vec::new(),
            vision: true,
            reasoning: true,
        })
        .collect())
}

fn supports_translation(id: &str) -> bool {
    let base = id
        .strip_prefix("ft:")
        .and_then(|id| id.split(':').next())
        .unwrap_or(id)
        .to_ascii_lowercase();
    let supported_family = base.starts_with("gpt-5")
        || base.starts_with("gpt-4.1")
        || base.starts_with("gpt-4o")
        || base == "o3"
        || base.starts_with("o3-");
    let incompatible = [
        "-audio",
        "-codex",
        "-deep-research",
        "-image",
        "-pro",
        "-realtime",
        "-search",
        "-transcribe",
        "-tts",
    ]
    .iter()
    .any(|marker| base.contains(marker));
    supported_family && !incompatible
}

pub(super) async fn translate(
    client: &Client,
    _config: &OpenAiConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get("openai")?.context("openai API key is not configured")?;
    let backend = ChatBackend {
        max_tokens: None,
        max_completion_tokens: generation.max_tokens,
        reasoning_effort: generation
            .reasoning
            .map(|enabled| if enabled { "medium" } else { "none" }),
        ..ChatBackend::new(
            "openai",
            URL,
            Some(api_key.expose_secret()),
            model,
            generation,
            ResponseMode::JsonSchema,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_models_to_the_chat_translation_contract() {
        assert!(supports_translation("gpt-5.6-luna"));
        assert!(supports_translation("gpt-4o-mini"));
        assert!(supports_translation("ft:gpt-4o-mini:org:name:id"));
        assert!(!supports_translation("gpt-5.5-pro"));
        assert!(!supports_translation("gpt-5.3-codex"));
        assert!(!supports_translation("gpt-4o-audio-preview"));
        assert!(!supports_translation("text-embedding-3-large"));
    }
}
