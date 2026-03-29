mod catalog;
mod runtime;
mod translation;

use std::str::FromStr;

use koharu_core::LlmModelInfo;
use koharu_llm::ModelId;
use koharu_llm::providers::{
    ProviderConfig, get_saved_api_key, openai_compatible, set_saved_api_key,
};
use tracing::instrument;

use super::{
    AppResources,
    request::{ApiKeyUpdate, LlmLoadJob, ModelCatalogQuery, TranslateJob},
    store::{self, ChangedField},
};

pub(crate) use runtime::LlmRuntime;

#[instrument(level = "debug", skip_all, fields(provider = %provider))]
pub(crate) async fn get_api_key(
    _state: AppResources,
    provider: &str,
) -> anyhow::Result<Option<String>> {
    match get_saved_api_key(provider) {
        Ok(value) => Ok(value),
        Err(err) => {
            tracing::error!(%err, "keyring read failed");
            Err(err)
        }
    }
}

#[instrument(level = "debug", skip_all, fields(provider = %update.provider))]
pub(crate) async fn set_api_key(_state: AppResources, update: ApiKeyUpdate) -> anyhow::Result<()> {
    match set_saved_api_key(&update.provider, &update.api_key) {
        Ok(()) => Ok(()),
        Err(err) => {
            tracing::error!(%err, "keyring write failed");
            Err(err)
        }
    }
}

pub(crate) async fn llm_list(
    state: AppResources,
    query: ModelCatalogQuery,
) -> anyhow::Result<Vec<LlmModelInfo>> {
    catalog::list_models(&state.llm, query).await
}

#[instrument(level = "info", skip_all)]
pub(crate) async fn llm_load(state: AppResources, job: LlmLoadJob) -> anyhow::Result<()> {
    if job.id.contains(':') {
        let (provider_id, model_id) = job.id.split_once(':').unwrap();
        let api_key = match job.api_key {
            Some(key) if !key.trim().is_empty() => Some(key),
            _ => get_saved_api_key(provider_id)?,
        };
        state
            .llm
            .load_api(
                provider_id,
                model_id,
                ProviderConfig {
                    api_key,
                    base_url: job.base_url,
                    temperature: job.temperature,
                    max_tokens: job.max_tokens,
                    custom_system_prompt: job.custom_system_prompt,
                },
            )
            .await?;
    } else {
        let id = ModelId::from_str(&job.id)?;
        state.llm.load_local(id).await;
    }
    Ok(())
}

pub(crate) async fn llm_offload(state: AppResources) -> anyhow::Result<()> {
    state.llm.offload().await;
    Ok(())
}

pub(crate) async fn llm_ready(state: AppResources) -> anyhow::Result<bool> {
    Ok(state.llm.ready().await)
}

#[instrument(level = "info", skip_all)]
pub(crate) async fn llm_generate(state: AppResources, job: TranslateJob) -> anyhow::Result<()> {
    let mut updated = store::read_doc(&state.state, job.document_index).await?;
    let target_language = job.language.as_deref();

    match job.text_block_index {
        Some(block_index) => {
            let text_block = updated
                .text_blocks
                .get_mut(block_index)
                .ok_or_else(|| anyhow::anyhow!("Text block not found"))?;
            state.llm.translate(text_block, target_language).await?;
        }
        None => {
            state.llm.translate(&mut updated, target_language).await?;
        }
    }

    store::update_doc(
        &state.state,
        job.document_index,
        updated,
        &[ChangedField::TextBlocks],
    )
    .await
}

pub(crate) async fn llm_ping(
    base_url: &str,
    api_key: Option<&str>,
) -> anyhow::Result<openai_compatible::PingResult> {
    openai_compatible::ping(base_url, api_key).await
}
