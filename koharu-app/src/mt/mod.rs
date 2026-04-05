//! Machine translation backends (DeepL, Google Cloud Translation).

mod deepl;
mod google;
pub mod lang;

use koharu_core::{DeeplTranslateOptions, TextBlock};
use koharu_llm::Language;
use koharu_llm::providers::get_saved_api_key;
use reqwest_middleware::ClientWithMiddleware;

use crate::AppResources;

pub use lang::{deepl_source_lang, deepl_target_lang, google_target_language};

/// Provider ids used in [`crate::config::PipelineConfig::translator`] and keyring.
pub const DEEPL_PROVIDER_ID: &str = "deepl";
pub const GOOGLE_TRANSLATE_PROVIDER_ID: &str = "google-translate";

/// Whether the pipeline translator id selects machine translation (no LLM).
pub fn is_machine_translator(translator_id: &str) -> bool {
    translator_id == DEEPL_PROVIDER_ID || translator_id == GOOGLE_TRANSLATE_PROVIDER_ID
}

/// API key (keyring or env) and optional base URL from app config for a translation provider.
pub async fn load_credentials(
    res: &AppResources,
    provider_id: &str,
) -> anyhow::Result<(String, Option<String>)> {
    let api_key = get_saved_api_key(provider_id)?
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("missing API key for machine translation ({provider_id})")
        })?;
    let base_url = {
        let config = res.config.read().await;
        config
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .and_then(|p| p.base_url.clone())
            .filter(|s| !s.trim().is_empty())
    };
    Ok((api_key, base_url))
}

/// Target locale from the active pipeline job ([`crate::pipeline::PipelineHandle::target_language`]).
pub async fn pipeline_target_language(res: &AppResources) -> Option<String> {
    res.pipeline
        .read()
        .await
        .as_ref()
        .and_then(|h| h.target_language.clone())
        .filter(|s| !s.trim().is_empty())
}

/// Apply translations to blocks in place: only updates `translation` for blocks with non-empty `text`.
#[allow(clippy::too_many_arguments)] // MT entrypoint: credentials + DeepL extras; refactor would touch all engines.
pub async fn translate_document_mt(
    client: &ClientWithMiddleware,
    translator_id: &str,
    api_key: &str,
    base_url: Option<&str>,
    blocks: &mut [TextBlock],
    target_language: Option<&str>,
    block_source_override: Option<usize>,
    deepl_options: Option<&DeeplTranslateOptions>,
) -> anyhow::Result<()> {
    let lang = target_language
        .and_then(Language::parse)
        .unwrap_or(Language::English);

    let indices: Vec<usize> = match block_source_override {
        Some(i) if i < blocks.len() => {
            if blocks[i]
                .text
                .as_deref()
                .map(str::trim)
                .is_some_and(|s| !s.is_empty())
            {
                vec![i]
            } else {
                vec![]
            }
        }
        _ => blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                b.text
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|s| !s.is_empty())
            })
            .map(|(i, _)| i)
            .collect(),
    };

    if indices.is_empty() {
        return Ok(());
    }

    let texts: Vec<String> = indices
        .iter()
        .map(|&i| blocks[i].text.clone().unwrap_or_default())
        .collect();

    let translated = match translator_id {
        id if id == DEEPL_PROVIDER_ID => {
            let tgt = deepl_target_lang(lang);
            let src = block_source_override
                .and_then(|i| blocks.get(i))
                .and_then(|b| deepl_source_lang(b.source_language.as_deref()))
                .or_else(|| {
                    blocks
                        .iter()
                        .find(|b| b.source_language.is_some())
                        .and_then(|b| deepl_source_lang(b.source_language.as_deref()))
                });
            deepl::translate_batch(client, api_key, base_url, &texts, tgt, src, deepl_options)
                .await?
        }
        id if id == GOOGLE_TRANSLATE_PROVIDER_ID => {
            let tgt = google_target_language(lang);
            let src = block_source_override
                .and_then(|i| blocks.get(i))
                .and_then(|b| b.source_language.as_deref().map(str::trim))
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    blocks
                        .iter()
                        .find_map(|b| b.source_language.as_deref().map(str::trim))
                        .filter(|s| !s.is_empty())
                });
            google::translate_batch(client, api_key, &texts, tgt, src).await?
        }
        _ => anyhow::bail!("unknown machine translator: {translator_id}"),
    };

    for (idx, &block_i) in indices.iter().enumerate() {
        if let Some(t) = translated.get(idx) {
            blocks[block_i].translation = Some(t.trim().to_string());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_translator_ids() {
        assert!(is_machine_translator(DEEPL_PROVIDER_ID));
        assert!(is_machine_translator(GOOGLE_TRANSLATE_PROVIDER_ID));
        assert!(!is_machine_translator("llm"));
    }
}
