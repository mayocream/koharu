// https://www.atlascloud.ai/docs/models/llm
// https://www.atlascloud.ai/blog/guides/deepseek-api-atlascloud-models-pricing-quickstart

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;

use super::openai_compatible::{ChatBackend, ResponseMode};
use crate::{GenerationConfig, Model, Provider, Result, TranslationRequest};

const CHAT_URL: &str = "https://api.atlascloud.ai/v1/chat/completions";
const MODELS_URL: &str = "https://api.atlascloud.ai/v1/models";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct AtlasCloudConfig {}

pub(super) async fn translate(
    client: &Client,
    _config: &AtlasCloudConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key =
        koharu_secrets::get("atlas-cloud")?.context("atlas-cloud API key is not configured")?;
    let backend = ChatBackend {
        reasoning: generation.reasoning,
        ..ChatBackend::new(
            "atlas-cloud",
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
    let Some(api_key) = koharu_secrets::get("atlas-cloud")? else {
        return Ok(Vec::new());
    };
    let discovered = super::openai_compatible::discover_models(
        "atlas-cloud",
        client.get(MODELS_URL).bearer_auth(api_key.expose_secret()),
    )
    .await;
    Ok(match discovered {
        Ok(discovered) => discovered
            .into_iter()
            .map(|model| Model {
                provider: Provider::AtlasCloud,
                name: crate::display_name(&model),
                model: Some(model),
                quantizations: Vec::new(),
                vision: false,
                reasoning: true,
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "failed to list Atlas Cloud models");
            Vec::new()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_include_the_required_v1_prefix() {
        assert_eq!(CHAT_URL, "https://api.atlascloud.ai/v1/chat/completions");
        assert_eq!(MODELS_URL, "https://api.atlascloud.ai/v1/models");
    }
}
