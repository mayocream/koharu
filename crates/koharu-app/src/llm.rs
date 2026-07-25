//! LLM lifecycle + translation driver.
//!
//! Owns the current LLM state (local llama.cpp model or remote provider).
//! Exposes `translate_texts(sources, target_lang, system_prompt)` which is
//! what the `llm-translate` pipeline engine calls.
//!
//! Construction:
//! ```ignore
//! let backend = app::shared_llama_backend(&runtime)?;
//! let llm = Arc::new(llm::Model::new(runtime, cpu, backend));
//! // then: llm.load_local(...) or llm.load_provider(...)
//! ```

use std::sync::Arc;

use anyhow::{Result, bail};
use koharu_core::{
    LlmCatalog, LlmCatalogModel, LlmLoadRequest, LlmProviderCatalog, LlmProviderCatalogStatus,
    LlmState, LlmStateStatus, LlmTarget, LlmTargetKind,
};
use koharu_llm::providers::{
    AnyProvider, ProviderCatalogModels, ProviderConfig, ProviderDescriptor,
    all_provider_descriptors, build_provider, discover_models,
};
use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_llm::{Language, Llm, ModelId, language::tags as language_tags};
use koharu_runtime::RuntimeManager;
use strum::IntoEnumIterator;
use tokio::sync::{RwLock, broadcast};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[allow(clippy::large_enum_variant)]
pub enum State {
    Empty,
    Loading {
        target: LlmTarget,
    },
    ReadyLocal(Llm),
    ReadyProvider {
        target: LlmTarget,
        /// `Arc`, not `Box`, so `translate_texts` can clone it out and drop
        /// the state lock *before* awaiting the provider's HTTP call.
        provider: Arc<dyn AnyProvider>,
    },
    Failed {
        target: Option<LlmTarget>,
        error: String,
    },
}

fn local_target(id: ModelId) -> LlmTarget {
    LlmTarget {
        kind: LlmTargetKind::Local,
        model_id: id.to_string(),
        provider_id: None,
    }
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
// Model
// ---------------------------------------------------------------------------

pub struct Model {
    state: Arc<RwLock<State>>,
    state_tx: broadcast::Sender<LlmState>,
    runtime: RuntimeManager,
    cpu: bool,
    backend: Arc<LlamaBackend>,
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

    /// Load a provider target (remote API) immediately.
    pub async fn load_provider(
        &self,
        target: LlmTarget,
        provider: Box<dyn AnyProvider>,
    ) -> Result<()> {
        *self.state.write().await = State::ReadyProvider {
            target,
            provider: Arc::from(provider),
        };
        self.emit_state().await;
        Ok(())
    }

    /// Kick off a local llama.cpp load in the background.
    pub async fn load_local(&self, id: ModelId) {
        let target = local_target(id);
        *self.state.write().await = State::Loading {
            target: target.clone(),
        };
        self.emit_state().await;

        let state_cloned = self.state.clone();
        let state_tx = self.state_tx.clone();
        let runtime = self.runtime.clone();
        let cpu = self.cpu;
        let backend = self.backend.clone();
        tokio::spawn(async move {
            let res = Llm::load(&runtime, id, cpu, backend).await;
            let mut guard = state_cloned.write().await;
            match res {
                Ok(llm) => *guard = State::ReadyLocal(llm),
                Err(e) => {
                    *guard = State::Failed {
                        target: Some(target),
                        error: format!("{e:#}"),
                    }
                }
            }
            let snapshot = snapshot_from_state(&guard);
            let _ = state_tx.send(snapshot);
        });
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
        state_target(&*self.state.read().await)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LlmState> {
        self.state_tx.subscribe()
    }

    pub async fn snapshot(&self) -> LlmState {
        snapshot_from_state(&*self.state.read().await)
    }

    async fn emit_state(&self) {
        let _ = self.state_tx.send(self.snapshot().await);
    }

    /// Run one generation against whichever backend is loaded.
    ///
    /// Remote providers are stateless, so the provider handle is cloned out
    /// under a *read* lock and the lock is released before the HTTP call is
    /// awaited — otherwise every translation in the process serializes on the
    /// state lock, and `snapshot()` / `ready()` block for the whole request.
    /// Only the local llama.cpp path takes the write lock, which it genuinely
    /// needs: `Llm::generate` is `&mut self` and a context is single-use.
    async fn generate_raw(
        &self,
        body: &str,
        target_language: Language,
        custom_system_prompt: Option<&str>,
    ) -> Result<String> {
        enum Route {
            Remote(Arc<dyn AnyProvider>, String),
            Local,
        }

        let route = {
            let guard = self.state.read().await;
            match &*guard {
                State::ReadyProvider { target, provider } => {
                    Route::Remote(provider.clone(), target.model_id.clone())
                }
                State::ReadyLocal(_) => Route::Local,
                State::Loading { .. } => bail!("LLM is still loading"),
                State::Failed { error, .. } => bail!("LLM failed to load: {error}"),
                State::Empty => bail!("no LLM loaded"),
            }
        };

        match route {
            Route::Remote(provider, model_id) => {
                provider
                    .translate(body, target_language, &model_id, custom_system_prompt)
                    .await
            }
            Route::Local => {
                let mut guard = self.state.write().await;
                match &mut *guard {
                    State::ReadyLocal(llm) => {
                        let opts = llm.id().default_generate_options();
                        llm.generate(body, &opts, target_language, custom_system_prompt)
                    }
                    // Raced with offload/reload between the read and write lock.
                    _ => bail!("no local LLM loaded"),
                }
            }
        }
    }

    /// Translate a batch of source strings. Each source becomes a tagged
    /// `[N]...` block; the response is parsed back into per-block
    /// translations. Output length matches input length (possibly with empty
    /// strings for missing blocks).
    pub async fn translate_texts(
        &self,
        sources: &[String],
        target_language: Option<&str>,
        custom_system_prompt: Option<&str>,
    ) -> Result<Vec<String>> {
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let target_language = target_language
            .and_then(Language::parse)
            .unwrap_or(Language::English);
        let body = format_sources(sources);

        let translation = self
            .generate_raw(&body, target_language, custom_system_prompt)
            .await?;

        let translation = strip_thinking_block(&translation);
        let out = match parse_tagged_blocks(translation, sources.len())? {
            Some(blocks) => blocks,
            None => split_legacy_lines(translation, sources.len()),
        };
        Ok(out
            .into_iter()
            .map(|s| strip_wrapping_quotes(s.trim()))
            .collect())
    }

    /// Translate several pages in a single request, tagging every block with
    /// its page (`[bPAGE-BLOCK]`) so the response can be validated.
    ///
    /// Returns one `Vec<String>` per input page, each the same length as that
    /// page's sources. If the response fails validation the pages are retried
    /// individually — slower, but it never lands a translation on the wrong
    /// bubble.
    ///
    /// Falls back to the per-page path (and the untouched `[N]` wire format)
    /// when there is nothing to gain or too much to risk: a single page, or a
    /// user-supplied system prompt that describes the old tag scheme.
    pub async fn translate_pages(
        &self,
        pages: &[Vec<String>],
        target_language: Option<&str>,
        custom_system_prompt: Option<&str>,
    ) -> Result<Vec<Vec<String>>> {
        let has_custom_prompt = custom_system_prompt.is_some_and(|p| !p.trim().is_empty());
        if pages.len() <= 1 || has_custom_prompt {
            return self
                .translate_each(pages, target_language, custom_system_prompt)
                .await;
        }

        let page_lens: Vec<usize> = pages.iter().map(|p| p.len()).collect();
        if page_lens.iter().all(|&n| n == 0) {
            return Ok(pages.iter().map(|_| Vec::new()).collect());
        }

        let language = target_language
            .and_then(Language::parse)
            .unwrap_or(Language::English);
        let refs: Vec<&[String]> = pages.iter().map(|p| p.as_slice()).collect();
        let body = format_sources_batched(&refs);

        let parsed = match self
            .generate_raw(&body, language, custom_system_prompt)
            .await
        {
            Ok(translation) => {
                let translation = strip_thinking_block(&translation);
                parse_batched_blocks(translation, &page_lens)
            }
            Err(err) => Err(err),
        };

        match parsed {
            Ok(blocks) => Ok(blocks
                .into_iter()
                .map(|page| {
                    page.into_iter()
                        .map(|s| strip_wrapping_quotes(s.trim()))
                        .collect()
                })
                .collect()),
            Err(err) => {
                tracing::warn!(
                    pages = pages.len(),
                    "batched translation rejected, retrying per page: {err:#}"
                );
                self.translate_each(pages, target_language, custom_system_prompt)
                    .await
            }
        }
    }

    /// Translate each page as its own request, preserving per-page failure
    /// isolation: one page erroring doesn't lose the others' translations.
    async fn translate_each(
        &self,
        pages: &[Vec<String>],
        target_language: Option<&str>,
        custom_system_prompt: Option<&str>,
    ) -> Result<Vec<Vec<String>>> {
        let mut out = Vec::with_capacity(pages.len());
        for sources in pages {
            out.push(
                self.translate_texts(sources, target_language, custom_system_prompt)
                    .await?,
            );
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Provider configuration + construction
// ---------------------------------------------------------------------------

impl Model {
    /// Resolve + build a provider from the app config, then load it.
    pub async fn load_from_request(
        &self,
        request: LlmLoadRequest,
        provider_config: Option<ProviderConfig>,
    ) -> Result<()> {
        match request.target.kind {
            LlmTargetKind::Local => {
                let id: ModelId =
                    std::str::FromStr::from_str(&request.target.model_id).map_err(|_| {
                        anyhow::anyhow!("unknown local model id: {}", request.target.model_id)
                    })?;
                self.load_local(id).await;
                Ok(())
            }
            LlmTargetKind::Provider => {
                let provider_id = request
                    .target
                    .provider_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("provider target missing provider_id"))?;
                let config = provider_config.ok_or_else(|| {
                    anyhow::anyhow!("no saved provider configuration for {provider_id}")
                })?;
                let provider = build_provider(provider_id, config)?;
                self.load_provider(request.target, provider).await?;
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

/// Build the LLM catalog (local models + providers). Dynamic-provider entries
/// perform a live model-discovery call when the provider has valid
/// configuration; Static providers always return the baked-in list.
pub async fn catalog(config: &crate::config::AppConfig, runtime: &RuntimeManager) -> LlmCatalog {
    LlmCatalog {
        local_models: local_catalog_models(),
        providers: provider_catalog(config, runtime).await,
    }
}

fn provider_target(provider_id: &str, model_id: &str) -> LlmTarget {
    LlmTarget {
        kind: LlmTargetKind::Provider,
        model_id: model_id.to_string(),
        provider_id: Some(provider_id.to_string()),
    }
}

fn local_catalog_models() -> Vec<LlmCatalogModel> {
    ModelId::iter()
        .map(|model| LlmCatalogModel {
            target: local_target(model),
            name: model.to_string(),
            languages: language_tags(&model.languages()),
        })
        .collect()
}

async fn provider_catalog(
    config: &crate::config::AppConfig,
    runtime: &RuntimeManager,
) -> Vec<LlmProviderCatalog> {
    let mut providers = Vec::new();
    for descriptor in all_provider_descriptors() {
        let stored = config.providers.iter().find(|p| p.id == descriptor.id);
        let base_url = stored.and_then(|p| p.base_url.clone());
        let api_key = stored
            .and_then(|p| p.api_key.as_ref())
            .map(|secret| secret.expose().to_owned());
        let has_api_key = api_key.as_deref().is_some_and(|v| !v.trim().is_empty());
        let missing = (descriptor.requires_api_key && !has_api_key)
            || (descriptor.requires_base_url
                && base_url.as_deref().is_none_or(|v| v.trim().is_empty()));

        let (status, error, models) = if missing {
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
                    let cfg = ProviderConfig {
                        http_client: runtime.http_client(),
                        api_key,
                        base_url: base_url.clone(),
                        temperature: None,
                        max_tokens: None,
                    };
                    match discover_models(descriptor.id, cfg) {
                        Ok(future) => match future.await {
                            Ok(discovered) => (
                                LlmProviderCatalogStatus::Ready,
                                None,
                                discovered
                                    .into_iter()
                                    .map(|m| LlmCatalogModel {
                                        target: provider_target(descriptor.id, &m.id),
                                        name: m.name,
                                        languages: descriptor.supported_languages.tags(),
                                    })
                                    .collect(),
                            ),
                            Err(e) => (
                                LlmProviderCatalogStatus::DiscoveryFailed,
                                Some(format!("{e:#}")),
                                Vec::new(),
                            ),
                        },
                        Err(e) => (
                            LlmProviderCatalogStatus::DiscoveryFailed,
                            Some(format!("{e:#}")),
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
    providers
}

fn static_provider_models(descriptor: &ProviderDescriptor) -> Vec<LlmCatalogModel> {
    match &descriptor.models {
        ProviderCatalogModels::Static(models) => models
            .iter()
            .map(|m| LlmCatalogModel {
                target: provider_target(descriptor.id, m.id),
                name: m.name.to_string(),
                languages: descriptor.supported_languages.tags(),
            })
            .collect(),
        ProviderCatalogModels::Dynamic(_) => Vec::new(),
    }
}

/// Build a `ProviderConfig` from stored app config. Used by `load_from_request`
/// when a provider target is requested.
pub fn provider_config_from_settings(
    config: &crate::config::AppConfig,
    runtime: &RuntimeManager,
    provider_id: &str,
) -> ProviderConfig {
    let stored = config.providers.iter().find(|p| p.id == provider_id);
    ProviderConfig {
        http_client: runtime.http_client(),
        api_key: stored
            .and_then(|p| p.api_key.as_ref())
            .map(|s| s.expose().to_owned()),
        base_url: stored.and_then(|p| p.base_url.clone()),
        temperature: None,
        max_tokens: None,
    }
}

// ---------------------------------------------------------------------------
// Tag formatting + response parsing
// ---------------------------------------------------------------------------

/// A parsed block tag, both indices 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BlockTag {
    page: usize,
    block: usize,
}

fn format_sources(sources: &[String]) -> String {
    sources
        .iter()
        .enumerate()
        .map(|(idx, text)| format!("[{}]{}", idx + 1, text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tag each block with its page as well as its index, so a response that
/// drops or repeats a tag can be *detected* rather than silently shifting
/// every later translation onto the wrong bubble — and, past a page boundary,
/// onto the wrong page.
fn format_sources_batched(pages: &[&[String]]) -> String {
    let mut lines = Vec::new();
    for (page_idx, sources) in pages.iter().enumerate() {
        for (block_idx, text) in sources.iter().enumerate() {
            lines.push(format!("[b{}-{}]{}", page_idx + 1, block_idx + 1, text));
        }
    }
    lines.join("\n")
}

/// Parse a leading block tag, accepting both the single-page `[N]` form
/// (implicitly page 1) and the batched `[bPAGE-BLOCK]` form. Returns the byte
/// length of the tag and the indices it names.
///
/// Accepting both is deliberate: a model that ignores the batched instruction
/// and replies with flat `[N]` still parses, and is then caught by
/// [`parse_batched_blocks`]'s validation rather than being mis-assigned.
fn parse_block_tag(text: &str) -> Option<(usize, BlockTag)> {
    if !text.starts_with('[') {
        return None;
    }
    let end = text[1..].find(']')?;
    let body = &text[1..1 + end];
    let len = 1 + end + 1;

    if let Some(rest) = body.strip_prefix(['b', 'B']) {
        let (page, block) = rest.split_once('-')?;
        let page: usize = page.parse().ok()?;
        let block: usize = block.parse().ok()?;
        if page == 0 || block == 0 {
            return None;
        }
        return Some((
            len,
            BlockTag {
                page: page - 1,
                block: block - 1,
            },
        ));
    }

    let block: usize = body.parse().ok()?;
    if block == 0 {
        return None;
    }
    Some((
        len,
        BlockTag {
            page: 0,
            block: block - 1,
        },
    ))
}

fn find_next_tag(text: &str) -> Option<(usize, usize, BlockTag)> {
    let mut line_start = 0;
    while line_start <= text.len() {
        let line = &text[line_start..];
        let indent = line
            .as_bytes()
            .iter()
            .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
            .count();
        let offset = line_start + indent;
        if let Some((len, tag)) = parse_block_tag(&text[offset..]) {
            return Some((offset, len, tag));
        }
        let Some(next_newline) = line.find('\n') else {
            break;
        };
        line_start += next_newline + 1;
    }
    None
}

/// Split a response into `(tag, content)` pairs in the order they appear.
fn scan_blocks(translation: &str) -> Vec<(BlockTag, String)> {
    let mut found = Vec::new();
    let mut cursor = translation;
    while let Some((offset, len, tag)) = find_next_tag(cursor) {
        cursor = &cursor[offset + len..];
        let content_end = find_next_tag(cursor)
            .map(|(next_offset, _, _)| next_offset)
            .unwrap_or(cursor.len());
        found.push((tag, cursor[..content_end].trim().to_string()));
        cursor = &cursor[content_end..];
    }
    found
}

fn parse_tagged_blocks(translation: &str, expected_blocks: usize) -> Result<Option<Vec<String>>> {
    let found = scan_blocks(translation);
    if found.is_empty() {
        return Ok(None);
    }
    let mut blocks = vec![String::new(); expected_blocks];
    for (tag, content) in found {
        if tag.page == 0 && tag.block < expected_blocks {
            blocks[tag.block] = content;
        }
    }
    Ok(Some(blocks))
}

/// Parse a batched response into per-page blocks.
///
/// Strict on purpose: every expected `(page, block)` must appear exactly once
/// and no unknown id may appear. `parse_tagged_blocks` can afford to be lenient
/// because a missing block just leaves one bubble empty; here a single dropped
/// tag would shift text across a page boundary, which is invisible to the user
/// and corrupts a page that looked fine. Callers retry rejected batches
/// page-by-page, trading a little speed for correctness.
fn parse_batched_blocks(translation: &str, page_lens: &[usize]) -> Result<Vec<Vec<String>>> {
    let found = scan_blocks(translation);
    if found.is_empty() {
        bail!("response contained no tagged blocks");
    }

    let mut slots: Vec<Vec<Option<String>>> =
        page_lens.iter().map(|&len| vec![None; len]).collect();

    for (tag, content) in found {
        let page = slots
            .get_mut(tag.page)
            .ok_or_else(|| anyhow::anyhow!("response referenced unknown page {}", tag.page + 1))?;
        let slot = page.get_mut(tag.block).ok_or_else(|| {
            anyhow::anyhow!(
                "response referenced unknown block {} on page {}",
                tag.block + 1,
                tag.page + 1
            )
        })?;
        if slot.is_some() {
            bail!(
                "response repeated block [b{}-{}]",
                tag.page + 1,
                tag.block + 1
            );
        }
        *slot = Some(content);
    }

    slots
        .into_iter()
        .enumerate()
        .map(|(page_idx, page)| {
            page.into_iter()
                .enumerate()
                .map(|(block_idx, slot)| {
                    slot.ok_or_else(|| {
                        anyhow::anyhow!(
                            "response missing block [b{}-{}]",
                            page_idx + 1,
                            block_idx + 1
                        )
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect()
}

fn split_legacy_lines(translation: &str, expected_blocks: usize) -> Vec<String> {
    let mut lines: Vec<String> = translation
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect();
    lines.truncate(expected_blocks);
    while lines.len() < expected_blocks {
        lines.push(String::new());
    }
    lines
}

fn strip_thinking_block(text: &str) -> &str {
    if let Some(start) = text.find("<think>")
        && let Some(end) = text[start..].find("</think>")
    {
        return text[start + end + "</think>".len()..].trim_start();
    }
    text
}

fn strip_wrapping_quotes(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.chars().next();
        let last = trimmed.chars().last();
        if let (Some(f), Some(l)) = (first, last)
            && (f == '"' && l == '"' || f == '\'' && l == '\'')
        {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(spec: &[&[&str]]) -> Vec<Vec<String>> {
        spec.iter()
            .map(|p| p.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    // --- wire format ------------------------------------------------------

    #[test]
    fn single_page_wire_format_is_unchanged() {
        let sources = vec!["one".to_string(), "two".to_string()];
        assert_eq!(format_sources(&sources), "[1]one\n[2]two");
    }

    #[test]
    fn batched_format_qualifies_every_tag_with_its_page() {
        let owned = pages(&[&["a", "b"], &["c"]]);
        let refs: Vec<&[String]> = owned.iter().map(|p| p.as_slice()).collect();
        assert_eq!(format_sources_batched(&refs), "[b1-1]a\n[b1-2]b\n[b2-1]c");
    }

    #[test]
    fn batched_round_trip() {
        let owned = pages(&[&["a", "b"], &["c"]]);
        let refs: Vec<&[String]> = owned.iter().map(|p| p.as_slice()).collect();
        let echoed = format_sources_batched(&refs);
        let parsed = parse_batched_blocks(&echoed, &[2, 1]).expect("round trip");
        assert_eq!(parsed, vec![vec!["a", "b"], vec!["c"]]);
    }

    // --- tag parsing ------------------------------------------------------

    #[test]
    fn parses_both_tag_forms() {
        assert_eq!(
            parse_block_tag("[3]hi").map(|(_, t)| t),
            Some(BlockTag { page: 0, block: 2 })
        );
        assert_eq!(
            parse_block_tag("[b2-3]hi").map(|(_, t)| t),
            Some(BlockTag { page: 1, block: 2 })
        );
        // Zero indices and malformed bodies are not tags.
        assert!(parse_block_tag("[0]x").is_none());
        assert!(parse_block_tag("[b0-1]x").is_none());
        assert!(parse_block_tag("[b1-0]x").is_none());
        assert!(parse_block_tag("[b1]x").is_none());
        assert!(parse_block_tag("[bx-y]x").is_none());
        assert!(parse_block_tag("no tag").is_none());
    }

    // --- validation: each of these must be REJECTED, not mis-assigned -----

    #[test]
    fn rejects_missing_block() {
        let err = parse_batched_blocks("[b1-1]a\n[b2-1]c", &[2, 1]).unwrap_err();
        assert!(err.to_string().contains("missing"), "{err}");
    }

    #[test]
    fn rejects_duplicated_block() {
        let err = parse_batched_blocks("[b1-1]a\n[b1-1]again\n[b2-1]c", &[1, 1]).unwrap_err();
        assert!(err.to_string().contains("repeated"), "{err}");
    }

    #[test]
    fn rejects_unknown_page() {
        let err = parse_batched_blocks("[b1-1]a\n[b9-1]ghost", &[1, 1]).unwrap_err();
        assert!(err.to_string().contains("unknown page"), "{err}");
    }

    #[test]
    fn rejects_unknown_block() {
        let err = parse_batched_blocks("[b1-1]a\n[b1-7]ghost\n[b2-1]c", &[1, 1]).unwrap_err();
        assert!(err.to_string().contains("unknown block"), "{err}");
    }

    #[test]
    fn rejects_flat_tags_when_batching() {
        // A model that ignores the batched format and replies with `[1] [2]`
        // would otherwise pile every block onto page 1.
        let err = parse_batched_blocks("[1]a\n[2]b", &[1, 1]).unwrap_err();
        assert!(err.to_string().contains("unknown block"), "{err}");
    }

    #[test]
    fn rejects_response_with_no_tags() {
        let err = parse_batched_blocks("just prose", &[1, 1]).unwrap_err();
        assert!(err.to_string().contains("no tagged blocks"), "{err}");
    }

    // --- ordering + content ----------------------------------------------

    #[test]
    fn accepts_tags_returned_out_of_order() {
        // Order on the wire doesn't matter; the tag says where each block goes.
        let parsed = parse_batched_blocks("[b2-1]c\n[b1-2]b\n[b1-1]a", &[2, 1]).unwrap();
        assert_eq!(parsed, vec![vec!["a", "b"], vec!["c"]]);
    }

    #[test]
    fn handles_pages_with_differing_and_zero_block_counts() {
        let parsed =
            parse_batched_blocks("[b1-1]a\n[b1-2]b\n[b1-3]c\n[b3-1]d", &[3, 0, 1]).unwrap();
        assert_eq!(parsed, vec![vec!["a", "b", "c"], vec![], vec!["d"]]);
    }

    #[test]
    fn keeps_multiline_block_content() {
        let parsed = parse_batched_blocks("[b1-1]line one\nline two\n[b2-1]c", &[1, 1]).unwrap();
        assert_eq!(parsed, vec![vec!["line one\nline two"], vec!["c"]]);
    }

    // --- single-page path is untouched ------------------------------------

    #[test]
    fn single_page_parse_stays_lenient() {
        // Unlike the batched parser, a missing block here just leaves that
        // bubble empty rather than failing the page.
        let parsed = parse_tagged_blocks("[1]a\n[3]c", 3).unwrap().unwrap();
        assert_eq!(parsed, vec!["a", "", "c"]);
    }

    #[test]
    fn single_page_parse_reports_untagged_response() {
        assert!(parse_tagged_blocks("no tags here", 2).unwrap().is_none());
    }
}
