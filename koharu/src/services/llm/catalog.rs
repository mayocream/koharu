use koharu_core::LlmModelInfo;
use koharu_llm::ModelId;
use koharu_llm::api::{ALL_API_PROVIDERS, OPENAI_COMPATIBLE_ID};
use koharu_llm::language::tags as language_tags;
use koharu_llm::providers::{get_saved_api_key, openai_compatible};
use koharu_llm::supported_locales;
use strum::IntoEnumIterator;

use crate::services::request::ModelCatalogQuery;

use super::runtime::LlmRuntime;

fn local_model_info(id: ModelId) -> LlmModelInfo {
    LlmModelInfo {
        id: id.to_string(),
        languages: language_tags(&id.languages()),
        source: "local".to_string(),
    }
}

fn api_model_info(provider_id: &'static str, model_id: &str) -> LlmModelInfo {
    LlmModelInfo {
        id: format!("{provider_id}:{model_id}"),
        languages: supported_locales(),
        source: provider_id.to_string(),
    }
}

pub(super) async fn list_models(
    runtime: &LlmRuntime,
    query: ModelCatalogQuery,
) -> anyhow::Result<Vec<LlmModelInfo>> {
    let mut models: Vec<ModelId> = ModelId::iter().collect();
    let cpu_factor = if runtime.is_cpu() { 10 } else { 1 };
    let lang = query.language.as_deref().unwrap_or("en");
    let zh_locale_factor = if lang.starts_with("zh") { 10 } else { 1 };
    let non_zh_en_locale_factor = if lang.starts_with("zh") || lang.starts_with("en") {
        1
    } else {
        100
    };

    models.sort_by_key(|model| match model {
        ModelId::VntlLlama3_8Bv2 => 100,
        ModelId::Lfm2_350mEnjpMt => 200 / cpu_factor,
        ModelId::SakuraGalTransl7Bv3_7 => 300 / zh_locale_factor,
        ModelId::Sakura1_5bQwen2_5v1_0 => 400 / zh_locale_factor / cpu_factor,
        ModelId::HunyuanMT7B => 500 / non_zh_en_locale_factor,
    });

    let mut result = models.into_iter().map(local_model_info).collect::<Vec<_>>();

    for provider in ALL_API_PROVIDERS {
        for model in provider.models {
            result.push(api_model_info(provider.id, model.id));
        }
    }

    if let Some(base_url) = query
        .openai_compatible_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let api_key = match get_saved_api_key(OPENAI_COMPATIBLE_ID) {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!(%err, "failed to read openai-compatible API key");
                None
            }
        };

        match openai_compatible::list_models(base_url, api_key.as_deref()).await {
            Ok(models) => {
                for model in models {
                    result.push(api_model_info(OPENAI_COMPATIBLE_ID, &model));
                }
            }
            Err(err) => {
                tracing::warn!(%err, "failed to list openai-compatible models");
            }
        }
    }

    Ok(result)
}
