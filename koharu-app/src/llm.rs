use std::str::FromStr;
use std::sync::Arc;

use koharu_llm::providers::{
    AnyProvider, ProviderCatalogModels, ProviderConfig, all_provider_descriptors, build_provider,
    discover_models,
};
use koharu_llm::providers::{DEEPL_ID, GOOGLE_TRANSLATE_ID};
use tokio::sync::{RwLock, broadcast};
use tracing::instrument;

use koharu_core::{
    Document, LlmCatalog, LlmCatalogModel, LlmGenerationOptions, LlmLoadRequest,
    LlmProviderCatalog, LlmProviderCatalogStatus, LlmState, LlmStateStatus, LlmTarget,
    LlmTargetKind, TextBlock,
};
use koharu_runtime::RuntimeManager;

use koharu_llm::{
    Language, Llm, ModelId, language::tags as language_tags, safe::llama_backend::LlamaBackend,
    supported_locales,
};
use strum::IntoEnumIterator;

use crate::AppResources;
use crate::config as app_config;

pub use koharu_llm::prefetch;

/// Matches a `[N]` tag, returning (full match length, 0-based index).
fn parse_block_tag(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    if bytes.first()? != &b'[' {
        return None;
    }
    let end = text[1..].find(']')?;
    let num_str = &text[1..1 + end];
    let id_1based: usize = num_str.parse().ok()?;
    if id_1based == 0 {
        return None;
    }
    Some((1 + end + 1, id_1based - 1))
}

#[allow(clippy::large_enum_variant)]
#[derive(strum::Display)]
pub enum State {
    #[strum(serialize = "empty")]
    Empty,
    #[strum(serialize = "loading")]
    Loading { target: LlmTarget },
    #[strum(serialize = "ready")]
    ReadyLocal(Llm),
    #[strum(serialize = "ready")]
    ReadyProvider {
        target: LlmTarget,
        provider: Box<dyn AnyProvider>,
    },
    #[strum(serialize = "failed")]
    Failed {
        target: Option<LlmTarget>,
        error: String,
    },
}

pub struct Model {
    state: Arc<RwLock<State>>,
    state_tx: broadcast::Sender<LlmState>,
    runtime: RuntimeManager,
    cpu: bool,
    backend: Arc<LlamaBackend>,
}

pub trait Translatable {
    fn get_source(&self) -> anyhow::Result<String>;
    fn set_translation(&mut self, translation: String) -> anyhow::Result<()>;
}

fn local_target(id: ModelId) -> LlmTarget {
    LlmTarget {
        kind: LlmTargetKind::Local,
        model_id: id.to_string(),
        provider_id: None,
    }
}

fn provider_target(provider_id: &str, model_id: &str) -> LlmTarget {
    LlmTarget {
        kind: LlmTargetKind::Provider,
        model_id: model_id.to_string(),
        provider_id: Some(provider_id.to_string()),
    }
}

fn validate_target(target: &LlmTarget) -> anyhow::Result<()> {
    match target.kind {
        LlmTargetKind::Local => {
            anyhow::ensure!(
                target.provider_id.is_none(),
                "local targets must not include provider_id"
            );
        }
        LlmTargetKind::Provider => {
            anyhow::ensure!(
                target
                    .provider_id
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()),
                "provider targets require provider_id"
            );
        }
    }

    anyhow::ensure!(
        !target.model_id.trim().is_empty(),
        "target model_id is required"
    );
    Ok(())
}

fn state_target(state: &State) -> Option<LlmTarget> {
    match state {
        State::Empty => None,
        State::Loading { target } => Some(target.clone()),
        State::ReadyLocal(llm) => Some(local_target(llm.id())),
        State::ReadyProvider { target, .. } => Some(target.clone()),
        State::Failed { target, .. } => target.clone(),
    }
}

fn strip_wrapping_quotes(text: &str) -> String {
    let mut current = text.trim();

    loop {
        let next = match current {
            _ if current.starts_with('"') && current.ends_with('"') => {
                current.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
            }
            _ if current.starts_with('\'') && current.ends_with('\'') => current
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\'')),
            _ if current.starts_with('“') && current.ends_with('”') => {
                current.strip_prefix('“').and_then(|s| s.strip_suffix('”'))
            }
            _ if current.starts_with('‘') && current.ends_with('’') => {
                current.strip_prefix('‘').and_then(|s| s.strip_suffix('’'))
            }
            _ => break,
        };
        let Some(next) = next else {
            break;
        };
        current = next.trim();
    }

    strip_incomplete_corner_quotes(current)
}

fn strip_incomplete_corner_quotes(text: &str) -> String {
    let mut current = text.trim();

    loop {
        let open_count = current.chars().filter(|&c| c == '「').count();
        let close_count = current.chars().filter(|&c| c == '」').count();

        if open_count > close_count && current.starts_with('「') {
            current = current.trim_start_matches('「').trim_start();
            continue;
        }

        if close_count > open_count && current.ends_with('」') {
            current = current.trim_end_matches('」').trim_end();
            continue;
        }

        break;
    }

    current.to_string()
}

/// Strip `<think>...</think>` reasoning blocks produced by thinking models.
fn strip_thinking_block(text: &str) -> &str {
    if let Some(start) = text.find("<think>")
        && let Some(end) = text[start..].find("</think>")
    {
        return text[start + end + "</think>".len()..].trim_start();
    }
    text
}

fn has_meaningful_text(text: Option<&str>) -> bool {
    text.is_some_and(|value| !value.trim().is_empty())
}

fn format_document_blocks(blocks: &[TextBlock]) -> String {
    blocks
        .iter()
        .enumerate()
        .map(|(idx, block)| {
            let text = block.text.as_deref().unwrap_or("");
            format!("[{}]{}", idx + 1, text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_next_tag(text: &str) -> Option<(usize, usize, usize)> {
    let mut line_start = 0;

    while line_start <= text.len() {
        let line = &text[line_start..];
        let indent = line
            .as_bytes()
            .iter()
            .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
            .count();
        let offset = line_start + indent;

        if let Some((len, id)) = parse_block_tag(&text[offset..]) {
            return Some((offset, len, id));
        }

        let Some(next_newline) = line.find('\n') else {
            break;
        };
        line_start += next_newline + 1;
    }

    None
}

fn parse_tagged_blocks(
    translation: &str,
    expected_blocks: usize,
) -> anyhow::Result<Option<Vec<String>>> {
    if find_next_tag(translation).is_none() {
        return Ok(None);
    }

    let mut blocks = vec![String::new(); expected_blocks];
    let mut cursor = translation;
    let mut found_any = false;
    let mut parsed_count = 0usize;
    let mut ignored_count = 0usize;

    while let Some((offset, len, id)) = find_next_tag(cursor) {
        found_any = true;
        cursor = &cursor[offset + len..];

        // Find content: everything until the next tag or end of string
        let content_end = find_next_tag(cursor)
            .map(|(next_offset, _, _)| next_offset)
            .unwrap_or(cursor.len());
        let content = cursor[..content_end].trim().to_string();

        if id >= expected_blocks {
            ignored_count += 1;
            tracing::warn!("Ignoring translated block id {id} for {expected_blocks} source blocks");
        } else {
            if blocks[id].is_empty() {
                parsed_count += 1;
            } else {
                tracing::warn!("Translated block id {id} appeared more than once, keeping latest");
            }
            blocks[id] = content;
        }

        cursor = &cursor[content_end..];
    }

    if !found_any {
        return Ok(None);
    }

    if parsed_count != expected_blocks || ignored_count != 0 {
        tracing::warn!(
            expected_blocks,
            parsed_count,
            ignored_count,
            output = translation,
            "translated block count mismatch"
        );
    }

    Ok(Some(blocks))
}

fn split_legacy_lines(translation: &str, expected_blocks: usize) -> anyhow::Result<Vec<String>> {
    let mut translations = translation
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect::<Vec<_>>();

    if translations.len() != expected_blocks {
        tracing::warn!(
            expected_blocks,
            got = translations.len(),
            output = translation,
            "translated line count mismatch"
        );
    }

    translations.truncate(expected_blocks);
    while translations.len() < expected_blocks {
        translations.push(String::new());
    }

    Ok(translations)
}

impl Translatable for Document {
    fn get_source(&self) -> anyhow::Result<String> {
        if !self
            .text_blocks
            .iter()
            .any(|block| has_meaningful_text(block.text.as_deref()))
        {
            return Ok(String::new());
        }
        Ok(format_document_blocks(&self.text_blocks))
    }

    fn set_translation(&mut self, translation: String) -> anyhow::Result<()> {
        let translation = strip_thinking_block(&translation);
        let translations = match parse_tagged_blocks(translation, self.text_blocks.len())? {
            Some(blocks) => blocks,
            None => split_legacy_lines(translation, self.text_blocks.len())?,
        };

        for (block, translation) in self.text_blocks.iter_mut().zip(translations) {
            block.translation = Some(strip_wrapping_quotes(&translation));
        }
        Ok(())
    }
}

impl Translatable for TextBlock {
    fn get_source(&self) -> anyhow::Result<String> {
        let source = self
            .text
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No source text found"))?;
        Ok(format!("[1]{}", source))
    }

    fn set_translation(&mut self, translation: String) -> anyhow::Result<()> {
        let translation = strip_thinking_block(&translation);
        let translation = match parse_tagged_blocks(translation, 1)? {
            Some(blocks) => blocks.into_iter().next().unwrap_or_default(),
            None => translation.to_string(),
        };
        self.translation = Some(strip_wrapping_quotes(&translation));
        Ok(())
    }
}

impl Model {
    pub fn new(runtime: RuntimeManager, cpu: bool, backend: Arc<LlamaBackend>) -> Self {
        Self {
            state: Arc::new(RwLock::new(State::Empty)),
            state_tx: broadcast::channel(64).0,
            runtime,
            cpu,
            backend,
        }
    }

    pub fn is_cpu(&self) -> bool {
        self.cpu
    }

    pub fn backend(&self) -> Arc<LlamaBackend> {
        self.backend.clone()
    }

    pub async fn load_provider(
        &self,
        target: LlmTarget,
        provider: Box<dyn AnyProvider>,
    ) -> anyhow::Result<()> {
        *self.state.write().await = State::ReadyProvider { target, provider };
        self.emit_state().await;
        Ok(())
    }

    pub async fn load_local(&self, id: ModelId) {
        let target = local_target(id);
        {
            let mut guard = self.state.write().await;
            *guard = State::Loading {
                target: target.clone(),
            };
        }
        self.emit_state().await;

        let state_cloned = self.state.clone();
        let state_tx = self.state_tx.clone();
        let runtime = self.runtime.clone();
        let cpu = self.cpu;
        let backend = self.backend.clone();
        tokio::spawn(async move {
            let res = Llm::load(&runtime, id, cpu, backend).await;
            match res {
                Ok(llm) => {
                    let mut guard = state_cloned.write().await;
                    *guard = State::ReadyLocal(llm);
                }
                Err(e) => {
                    tracing::error!("LLM load join error: {e}");
                    let mut guard = state_cloned.write().await;
                    *guard = State::Failed {
                        target: Some(target),
                        error: format!("join error: {e}"),
                    };
                }
            }
            let snapshot = {
                let guard = state_cloned.read().await;
                snapshot_from_state(&guard)
            };
            let _ = state_tx.send(snapshot);
        });
    }

    pub async fn get(&self) -> tokio::sync::RwLockReadGuard<'_, State> {
        self.state.read().await
    }

    pub async fn get_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, State> {
        self.state.write().await
    }

    pub async fn offload(&self) {
        *self.state.write().await = State::Empty;
        self.emit_state().await;
    }

    pub async fn ready(&self) -> bool {
        matches!(
            *self.state.read().await,
            State::ReadyLocal(_) | State::ReadyProvider { .. }
        )
    }

    pub async fn current_target(&self) -> Option<LlmTarget> {
        let guard = self.state.read().await;
        state_target(&guard)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LlmState> {
        self.state_tx.subscribe()
    }

    pub async fn snapshot(&self) -> LlmState {
        let guard = self.state.read().await;
        snapshot_from_state(&guard)
    }

    async fn emit_state(&self) {
        let _ = self.state_tx.send(self.snapshot().await);
    }

    pub async fn translate(
        &self,
        doc: &mut impl Translatable,
        target_language: Option<&str>,
        custom_system_prompt: Option<&str>,
    ) -> anyhow::Result<()> {
        let target_language = target_language
            .and_then(Language::parse)
            .unwrap_or(Language::English);
        let source = doc.get_source()?;
        if source.is_empty() {
            tracing::debug!("skipping translate: no source text");
            return Ok(());
        }
        let mut guard = self.state.write().await;
        let translation = match &mut *guard {
            State::ReadyLocal(llm) => {
                let opts = llm.id().default_generate_options();
                llm.generate(&source, &opts, target_language, custom_system_prompt)
            }
            State::ReadyProvider { target, provider } => {
                provider
                    .translate(
                        &source,
                        target_language,
                        &target.model_id,
                        custom_system_prompt,
                    )
                    .await
            }
            State::Loading { .. } => Err(anyhow::anyhow!("Model is still loading")),
            State::Failed { error, .. } => Err(anyhow::anyhow!("Model failed to load: {error}")),
            State::Empty => Err(anyhow::anyhow!("No model is loaded")),
        }?;
        doc.set_translation(translation.trim().to_string())
    }
}

fn snapshot_from_state(state: &State) -> LlmState {
    match state {
        State::Empty => LlmState {
            status: LlmStateStatus::Empty,
            target: None,
            error: None,
        },
        State::Loading { target } => LlmState {
            status: LlmStateStatus::Loading,
            target: Some(target.clone()),
            error: None,
        },
        State::ReadyLocal(llm) => LlmState {
            status: LlmStateStatus::Ready,
            target: Some(local_target(llm.id())),
            error: None,
        },
        State::ReadyProvider { target, .. } => LlmState {
            status: LlmStateStatus::Ready,
            target: Some(target.clone()),
            error: None,
        },
        State::Failed { target, error } => LlmState {
            status: LlmStateStatus::Failed,
            target: target.clone(),
            error: Some(error.clone()),
        },
    }
}

// ---------------------------------------------------------------------------
// Operations (merged from ops/llm.rs)
// ---------------------------------------------------------------------------

fn local_catalog_models() -> Vec<LlmCatalogModel> {
    ModelId::iter()
        .map(|model| LlmCatalogModel {
            target: local_target(model),
            name: model.to_string(),
            languages: language_tags(&model.languages()),
        })
        .collect()
}

async fn provider_catalog(state: &AppResources) -> anyhow::Result<Vec<LlmProviderCatalog>> {
    let config = app_config::load()?;
    let mut providers = Vec::new();

    for descriptor in all_provider_descriptors() {
        let stored = config.providers.iter().find(|p| p.id == descriptor.id);
        let base_url = stored.and_then(|p| p.base_url.clone());
        let api_key = stored
            .and_then(|p| p.api_key.as_ref())
            .map(|secret| secret.expose().to_owned());
        let has_api_key = api_key
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        let missing_required_config = (descriptor.requires_api_key && !has_api_key)
            || (descriptor.requires_base_url
                && base_url
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty()));

        let (status, error, models) = if missing_required_config {
            (
                LlmProviderCatalogStatus::MissingConfiguration,
                None,
                static_provider_models(descriptor),
            )
        } else {
            match &descriptor.models {
                ProviderCatalogModels::Static(_) => (
                    LlmProviderCatalogStatus::Ready,
                    None,
                    static_provider_models(descriptor),
                ),
                ProviderCatalogModels::Dynamic(_) => {
                    let models = discover_models(
                        descriptor.id,
                        ProviderConfig {
                            http_client: state.runtime.http_client(),
                            http_client_raw: state.runtime.http_client_raw(),
                            api_key,
                            base_url: base_url.clone(),
                            temperature: None,
                            max_tokens: None,
                        },
                    )?
                    .await;

                    match models {
                        Ok(models) => (
                            LlmProviderCatalogStatus::Ready,
                            None,
                            models
                                .into_iter()
                                .map(|model| LlmCatalogModel {
                                    target: provider_target(descriptor.id, &model.id),
                                    name: model.name,
                                    languages: supported_locales(),
                                })
                                .collect(),
                        ),
                        Err(error) => (
                            LlmProviderCatalogStatus::DiscoveryFailed,
                            Some(error.to_string()),
                            Vec::new(),
                        ),
                    }
                }
            }
        };

        providers.push(LlmProviderCatalog {
            id: descriptor.id.to_string(),
            name: descriptor.name.to_string(),
            requires_api_key: descriptor.requires_api_key,
            requires_base_url: descriptor.requires_base_url,
            has_api_key,
            base_url,
            status,
            error,
            models,
        });
    }

    Ok(providers)
}

fn static_provider_models(
    descriptor: &koharu_llm::providers::ProviderDescriptor,
) -> Vec<LlmCatalogModel> {
    match &descriptor.models {
        ProviderCatalogModels::Static(models) => models
            .iter()
            .map(|model| LlmCatalogModel {
                target: provider_target(descriptor.id, model.id),
                name: model.name.to_string(),
                languages: supported_locales(),
            })
            .collect(),
        ProviderCatalogModels::Dynamic(_) => Vec::new(),
    }
}

fn provider_config_from_settings(
    state: &AppResources,
    target: &LlmTarget,
    options: Option<&LlmGenerationOptions>,
) -> anyhow::Result<ProviderConfig> {
    validate_target(target)?;
    let provider_id = target
        .provider_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("provider targets require provider_id"))?;
    let config = app_config::load()?;
    let stored = config.providers.iter().find(|p| p.id == provider_id);

    Ok(ProviderConfig {
        http_client: state.runtime.http_client(),
        http_client_raw: state.runtime.http_client_raw(),
        api_key: stored
            .and_then(|p| p.api_key.as_ref())
            .map(|secret| secret.expose().to_owned()),
        base_url: stored.and_then(|p| p.base_url.clone()),
        temperature: options.and_then(|options| options.temperature),
        max_tokens: options.and_then(|options| options.max_tokens),
    })
}

async fn load_target(
    state: &AppResources,
    target: &LlmTarget,
    options: Option<&LlmGenerationOptions>,
) -> anyhow::Result<()> {
    validate_target(target)?;
    if state.llm.current_target().await.as_ref() == Some(target) && state.llm.ready().await {
        return Ok(());
    }

    match target.kind {
        LlmTargetKind::Local => {
            let model = ModelId::from_str(&target.model_id)?;
            state.llm.load_local(model).await;
        }
        LlmTargetKind::Provider => {
            let provider_id = target
                .provider_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("provider targets require provider_id"))?;
            let provider = build_provider(
                provider_id,
                provider_config_from_settings(state, target, options)?,
            )?;
            state.llm.load_provider(target.clone(), provider).await?;
        }
    }

    Ok(())
}

pub async fn llm_catalog(state: AppResources) -> anyhow::Result<LlmCatalog> {
    Ok(LlmCatalog {
        local_models: local_catalog_models(),
        providers: provider_catalog(&state).await?,
    })
}

#[instrument(level = "info", skip_all)]
pub async fn llm_load(state: AppResources, payload: LlmLoadRequest) -> anyhow::Result<()> {
    load_target(&state, &payload.target, payload.options.as_ref()).await
}

pub async fn llm_offload(state: AppResources) -> anyhow::Result<()> {
    state.llm.offload().await;
    Ok(())
}

pub async fn llm_ready(state: AppResources) -> anyhow::Result<bool> {
    Ok(state.llm.ready().await)
}

#[instrument(level = "info", skip_all)]
pub async fn llm_generate(
    state: AppResources,
    document_id: &str,
    text_block_index: Option<usize>,
    language: Option<&str>,
    system_prompt: Option<&str>,
) -> anyhow::Result<()> {
    let mut doc = state.storage.page(document_id).await?;

    let translator = {
        let cfg = state.config.read().await;
        cfg.pipeline.translator.clone()
    };

    match text_block_index {
        Some(block_index) => {
            let text_block = doc
                .text_blocks
                .get_mut(block_index)
                .ok_or_else(|| anyhow::anyhow!("Text block not found"))?;
            if translator == DEEPL_ID || translator == GOOGLE_TRANSLATE_ID {
                let cfg = app_config::load()?;
                let stored = cfg.providers.iter().find(|p| p.id == translator);
                let api_key = stored
                    .and_then(|p| p.api_key.as_ref())
                    .map(|secret| secret.expose().to_owned());
                let base_url = stored.and_then(|p| p.base_url.clone());
                let provider = build_provider(
                    &translator,
                    ProviderConfig {
                        http_client: state.runtime.http_client(),
                        http_client_raw: state.runtime.http_client_raw(),
                        api_key,
                        base_url,
                        temperature: None,
                        max_tokens: None,
                    },
                )?;

                let lang = language
                    .and_then(Language::parse)
                    .unwrap_or(Language::English);
                let src = text_block.text.as_deref().map(str::trim).unwrap_or("");
                if !src.is_empty() {
                    let t = provider.translate(src, lang, "default", None).await?;
                    text_block.translation = Some(t.trim().to_string());
                }
            } else {
                state
                    .llm
                    .translate(text_block, language, system_prompt)
                    .await?;
            }
        }
        None => {
            state
                .llm
                .translate(&mut doc, language, system_prompt)
                .await?;
        }
    }

    let text_blocks = doc.text_blocks;
    state
        .storage
        .update_page(document_id, |page| {
            page.text_blocks = text_blocks;
        })
        .await
}

pub async fn get_document_for_llm(
    state: AppResources,
    document_id: &str,
) -> anyhow::Result<koharu_core::Document> {
    state.storage.page(document_id).await
}

#[cfg(test)]
mod tests {
    use koharu_core::Document;

    use super::*;

    #[test]
    fn document_source_uses_tagged_blocks() -> anyhow::Result<()> {
        let doc = Document {
            text_blocks: vec![
                TextBlock {
                    text: Some("Hello".to_string()),
                    ..Default::default()
                },
                TextBlock {
                    text: Some("1 < 2\nA & B".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let source = doc.get_source()?;
        assert_eq!(source, "[1]Hello\n[2]1 < 2\nA & B");

        Ok(())
    }

    #[test]
    fn document_translation_parses_tagged_blocks_by_id() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation("[2]Second line\nnext\n[1]First <done>".to_string())?;

        assert_eq!(
            doc.text_blocks[0].translation.as_deref(),
            Some("First <done>")
        );
        assert_eq!(
            doc.text_blocks[1].translation.as_deref(),
            Some("Second line\nnext")
        );

        Ok(())
    }

    #[test]
    fn document_translation_strips_wrapping_quotes() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation("[1]\"Hello\"\n[2]\u{201c}World\u{201d}".to_string())?;

        assert_eq!(doc.text_blocks[0].translation.as_deref(), Some("Hello"));
        assert_eq!(doc.text_blocks[1].translation.as_deref(), Some("World"));

        Ok(())
    }

    #[test]
    fn document_translation_pads_mismatched_legacy_lines() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation("only one line".to_string())?;

        assert_eq!(
            doc.text_blocks[0].translation.as_deref(),
            Some("only one line")
        );
        assert_eq!(doc.text_blocks[1].translation.as_deref(), Some(""));

        Ok(())
    }

    #[test]
    fn document_translation_parses_consecutive_tags() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation("[1]First line\n[2]Second line".to_string())?;

        assert_eq!(
            doc.text_blocks[0].translation.as_deref(),
            Some("First line")
        );
        assert_eq!(
            doc.text_blocks[1].translation.as_deref(),
            Some("Second line")
        );

        Ok(())
    }

    #[test]
    fn document_translation_uses_end_of_text_for_last_block() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation("[1]Final line".to_string())?;

        assert_eq!(
            doc.text_blocks[0].translation.as_deref(),
            Some("Final line")
        );

        Ok(())
    }

    #[test]
    fn document_translation_ignores_out_of_range_tagged_blocks() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation("[1]Kept\n[2]Ignored".to_string())?;

        assert_eq!(doc.text_blocks[0].translation.as_deref(), Some("Kept"));

        Ok(())
    }

    #[test]
    fn document_translation_pads_missing_tagged_blocks() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation("[1]Only first".to_string())?;

        assert_eq!(
            doc.text_blocks[0].translation.as_deref(),
            Some("Only first")
        );
        assert_eq!(doc.text_blocks[1].translation.as_deref(), Some(""));

        Ok(())
    }

    #[test]
    fn text_block_translation_strips_wrapping_quotes() -> anyhow::Result<()> {
        let mut block = TextBlock::default();
        block.set_translation("\u{201c}quoted\u{201d}".to_string())?;
        assert_eq!(block.translation.as_deref(), Some("quoted"));
        Ok(())
    }

    #[test]
    fn text_block_source_uses_single_tagged_block() -> anyhow::Result<()> {
        let block = TextBlock {
            text: Some("1 < 2\nA & B".to_string()),
            ..Default::default()
        };

        let source = block.get_source()?;
        assert_eq!(source, "[1]1 < 2\nA & B");

        Ok(())
    }

    #[test]
    fn document_source_skips_translation_when_all_blocks_are_empty() -> anyhow::Result<()> {
        let doc = Document {
            text_blocks: vec![
                TextBlock::default(),
                TextBlock {
                    text: Some("   ".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        assert_eq!(doc.get_source()?, "");
        Ok(())
    }

    #[test]
    fn text_block_translation_extracts_tagged_block_content() -> anyhow::Result<()> {
        let mut block = TextBlock::default();
        block.set_translation("Sure.\n[1]Translated text".to_string())?;
        assert_eq!(block.translation.as_deref(), Some("Translated text"));
        Ok(())
    }

    #[test]
    fn text_block_translation_keeps_multiline_plain_text() -> anyhow::Result<()> {
        let mut block = TextBlock::default();
        block.set_translation("First line\nSecond line".to_string())?;
        assert_eq!(
            block.translation.as_deref(),
            Some("First line\nSecond line")
        );
        Ok(())
    }

    #[test]
    fn text_block_translation_keeps_japanese_dialogue_quotes() -> anyhow::Result<()> {
        let mut block = TextBlock::default();
        block.set_translation("\u{300c}quoted\u{300d}".to_string())?;
        assert_eq!(block.translation.as_deref(), Some("\u{300c}quoted\u{300d}"));
        Ok(())
    }

    #[test]
    fn parse_block_tag_parses_valid_tags() {
        assert_eq!(parse_block_tag("[1]"), Some((3, 0)));
        assert_eq!(parse_block_tag("[2]"), Some((3, 1)));
        assert_eq!(parse_block_tag("[10]"), Some((4, 9)));
    }

    #[test]
    fn parse_block_tag_rejects_invalid_tags() {
        assert_eq!(parse_block_tag("[0]"), None);
        assert_eq!(parse_block_tag("[abc]"), None);
        assert_eq!(parse_block_tag("<block>"), None);
        assert_eq!(parse_block_tag("hello"), None);
    }

    #[test]
    fn strip_thinking_block_removes_think_tags() {
        assert_eq!(
            strip_thinking_block("<think>reasoning here</think>[1]Hello"),
            "[1]Hello"
        );
        assert_eq!(strip_thinking_block("no thinking here"), "no thinking here");
        assert_eq!(
            strip_thinking_block("<think>long\nmultiline\nthought</think>\n[1]Result"),
            "[1]Result"
        );
    }

    #[test]
    fn parse_tagged_blocks_ignores_inline_analysis_tags() -> anyhow::Result<()> {
        let parsed =
            parse_tagged_blocks("1. Analyze the Input Data:\n* [1] `1`\n* [2] `ねむ……`", 2)?;
        assert_eq!(parsed, None);

        Ok(())
    }

    #[test]
    fn parse_tagged_blocks_accepts_real_tagged_lines_after_preamble() -> anyhow::Result<()> {
        let parsed = parse_tagged_blocks("Sure.\n[1]Hello\n[2]World", 2)?;
        assert_eq!(parsed, Some(vec!["Hello".to_string(), "World".to_string()]));

        Ok(())
    }
}
