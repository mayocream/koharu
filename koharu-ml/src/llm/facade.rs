use std::sync::Arc;

use serde::Serialize;
use tokio::sync::RwLock;

use koharu_types::{Document, TextBlock};

use super::{GenerateOptions, Llm, ModelId};

pub use super::prefetch;

const BLOCK_START_PREFIX: &str = r#"<block id=""#;
const BLOCK_START_SUFFIX: &str = r#"">"#;
const BLOCK_END: &str = "</block>";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub languages: Vec<String>,
    pub source: &'static str,
}

impl ModelInfo {
    pub fn new(id: ModelId) -> Self {
        Self {
            id: id.to_string(),
            languages: id.languages(),
            source: "local",
        }
    }

    pub fn api(provider_id: &'static str, model_id: &str) -> Self {
        Self {
            id: format!("{provider_id}:{model_id}"),
            languages: vec![],
            source: provider_id,
        }
    }
}

/// Load state of the LLM
#[allow(clippy::large_enum_variant)]
pub enum State {
    Empty,
    Loading,
    Ready(Llm),
    ApiReady {
        provider: Box<dyn super::provider::AnyProvider>,
        model: String,
    },
    Failed(String),
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            State::Empty => write!(f, "empty"),
            State::Loading => write!(f, "loading"),
            State::Ready(_) | State::ApiReady { .. } => write!(f, "ready"),
            State::Failed(_) => write!(f, "failed"),
        }
    }
}

/// Minimal owner for the LLM with non-blocking initialization.
pub struct Model {
    state: Arc<RwLock<State>>,
    use_cpu: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self::new(false)
    }
}

pub trait Translatable {
    fn get_source(&self) -> anyhow::Result<String>;
    fn set_translation(&mut self, translation: String) -> anyhow::Result<()>;

    fn translate_with_llm(
        &mut self,
        llm: &mut Llm,
        target_language: Option<&str>,
    ) -> anyhow::Result<()> {
        let text = self.get_source()?;
        let response = llm.generate(&text, &GenerateOptions::default(), target_language)?;
        let response = response.trim().to_string();
        self.set_translation(response)
    }
}

fn escape_block_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn unescape_block_text(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
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

    current.to_string()
}

fn format_document_blocks(blocks: &[TextBlock]) -> String {
    blocks
        .iter()
        .enumerate()
        .map(|(idx, block)| {
            let text = block.text.as_deref().unwrap_or("<empty>");
            format!(
                r#"<block id="{idx}">
{}
</block>"#,
                escape_block_text(text)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_tagged_blocks(
    translation: &str,
    expected_blocks: usize,
) -> anyhow::Result<Option<Vec<String>>> {
    if !translation.contains(BLOCK_START_PREFIX) {
        return Ok(None);
    }

    let mut blocks = vec![String::new(); expected_blocks];
    let mut cursor = translation;
    let mut found_any = false;
    let mut parsed_count = 0usize;
    let mut ignored_count = 0usize;

    while let Some(start_idx) = cursor.find(BLOCK_START_PREFIX) {
        found_any = true;
        cursor = &cursor[start_idx + BLOCK_START_PREFIX.len()..];

        let id_end = cursor
            .find(BLOCK_START_SUFFIX)
            .ok_or_else(|| anyhow::anyhow!("Malformed translated block: missing block header"))?;
        let id = cursor[..id_end]
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Malformed translated block id"))?;
        if id >= expected_blocks {
            ignored_count += 1;
            tracing::warn!("Ignoring translated block id {id} for {expected_blocks} source blocks");
            cursor = &cursor[id_end + BLOCK_START_SUFFIX.len()..];
            let boundary = block_boundary(cursor, cursor.find(BLOCK_END));
            cursor = if cursor.find(BLOCK_END) == Some(boundary) {
                &cursor[boundary + BLOCK_END.len()..]
            } else {
                &cursor[boundary..]
            };
            continue;
        }

        cursor = &cursor[id_end + BLOCK_START_SUFFIX.len()..];

        let closing_tag = cursor.find(BLOCK_END);
        let block_end = block_boundary(cursor, closing_tag);
        let content = unescape_block_text(cursor[..block_end].trim());

        if blocks[id].is_empty() {
            parsed_count += 1;
        } else {
            tracing::warn!("Translated block id {id} appeared more than once, keeping latest");
        }
        blocks[id] = content;

        cursor = if closing_tag == Some(block_end) {
            &cursor[block_end + BLOCK_END.len()..]
        } else {
            &cursor[block_end..]
        };
    }

    if !found_any {
        return Ok(None);
    }

    if parsed_count != expected_blocks || ignored_count != 0 {
        tracing::warn!(
            "Translated block count mismatch: expected {expected_blocks}, got {parsed_count}, ignored {ignored_count}"
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
            "Translated line count mismatch: expected {expected_blocks}, got {}",
            translations.len()
        );
    }

    translations.truncate(expected_blocks);
    while translations.len() < expected_blocks {
        translations.push(String::new());
    }

    Ok(translations)
}

fn block_boundary(cursor: &str, closing_tag: Option<usize>) -> usize {
    let next_block_start = cursor.find(BLOCK_START_PREFIX);
    match (closing_tag, next_block_start) {
        (Some(close), Some(next)) => close.min(next),
        (Some(close), None) => close,
        (None, Some(next)) => next,
        (None, None) => cursor.len(),
    }
}

impl Translatable for Document {
    fn get_source(&self) -> anyhow::Result<String> {
        Ok(format_document_blocks(&self.text_blocks))
    }

    fn set_translation(&mut self, translation: String) -> anyhow::Result<()> {
        let translations = match parse_tagged_blocks(&translation, self.text_blocks.len())? {
            Some(blocks) => blocks,
            None => split_legacy_lines(&translation, self.text_blocks.len())?,
        };

        for (block, translation) in self.text_blocks.iter_mut().zip(translations) {
            block.translation = Some(strip_wrapping_quotes(&translation));
        }
        Ok(())
    }

    fn translate_with_llm(
        &mut self,
        llm: &mut Llm,
        target_language: Option<&str>,
    ) -> anyhow::Result<()> {
        let text = self.get_source()?;
        let response = llm.generate(&text, &GenerateOptions::default(), target_language)?;
        let response = response.trim().to_string();
        self.set_translation(response)
    }
}

impl Translatable for TextBlock {
    fn get_source(&self) -> anyhow::Result<String> {
        let source = self
            .text
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No source text found"))?;
        Ok(source)
    }

    fn set_translation(&mut self, translation: String) -> anyhow::Result<()> {
        self.translation = Some(strip_wrapping_quotes(&translation));
        Ok(())
    }
}

impl Model {
    pub fn new(use_cpu: bool) -> Self {
        Self {
            state: Arc::new(RwLock::new(State::Empty)),
            use_cpu,
        }
    }

    pub fn is_cpu(&self) -> bool {
        self.use_cpu
    }

    /// Start loading an API-backed provider and return immediately.
    pub async fn load_api(
        &self,
        provider_id: &str,
        model_id: &str,
        api_key: String,
    ) -> anyhow::Result<()> {
        use super::provider::AnyProvider;
        let provider: Box<dyn AnyProvider> = match provider_id {
            "openai" => Box::new(super::provider::openai::OpenAiProvider { api_key }),
            "gemini" => Box::new(super::provider::gemini::GeminiProvider { api_key }),
            "claude" => Box::new(super::provider::claude::ClaudeProvider { api_key }),
            other => anyhow::bail!("Unknown API provider: {other}"),
        };
        *self.state.write().await = State::ApiReady {
            provider,
            model: model_id.to_string(),
        };
        Ok(())
    }

    /// Start loading the model on a blocking thread and return immediately.
    pub async fn load(&self, id: ModelId) {
        {
            let mut guard = self.state.write().await;
            *guard = State::Loading;
        }

        let state_cloned = self.state.clone();
        let use_cpu = self.use_cpu;
        tokio::spawn(async move {
            let res = Llm::load(id, use_cpu).await;
            match res {
                Ok(llm) => {
                    let mut guard = state_cloned.write().await;
                    *guard = State::Ready(llm);
                }
                Err(e) => {
                    tracing::error!("LLM load join error: {e}");
                    let mut guard = state_cloned.write().await;
                    *guard = State::Failed(format!("join error: {e}"));
                }
            }
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
    }

    pub async fn ready(&self) -> bool {
        matches!(
            *self.state.read().await,
            State::Ready(_) | State::ApiReady { .. }
        )
    }

    pub async fn translate(
        &self,
        doc: &mut impl Translatable,
        target_language: Option<&str>,
    ) -> anyhow::Result<()> {
        let lang = target_language.unwrap_or("English");
        let mut guard = self.state.write().await;
        match &mut *guard {
            State::Ready(llm) => doc.translate_with_llm(llm, target_language),
            State::ApiReady { provider, model } => {
                let text = doc.get_source()?;
                let model = model.clone();
                let response = provider.translate(&text, lang, &model).await?;
                let response = response.trim().to_string();
                doc.set_translation(response)
            }
            State::Loading => Err(anyhow::anyhow!("Model is still loading")),
            State::Failed(e) => Err(anyhow::anyhow!("Model failed to load: {e}")),
            State::Empty => Err(anyhow::anyhow!("No model is loaded")),
        }
    }
}

#[cfg(test)]
mod tests {
    use koharu_types::Document;

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
        assert_eq!(
            source,
            "<block id=\"0\">\nHello\n</block>\n<block id=\"1\">\n1 &lt; 2\nA &amp; B\n</block>"
        );

        Ok(())
    }

    #[test]
    fn document_translation_parses_tagged_blocks_by_id() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation(
            "<block id=\"1\">\nSecond line\nnext\n</block>\n<block id=\"0\">\nFirst &lt;done&gt;\n</block>".to_string(),
        )?;

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

        doc.set_translation(
            "<block id=\"0\">\n\"Hello\"\n</block>\n<block id=\"1\">\n“World”\n</block>"
                .to_string(),
        )?;

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
    fn document_translation_allows_missing_closing_tags() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation(
            "<block id=\"0\">\nFirst line\n<block id=\"1\">\nSecond line".to_string(),
        )?;

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
    fn document_translation_uses_end_of_text_when_last_closing_tag_is_missing() -> anyhow::Result<()>
    {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation("<block id=\"0\">\nFinal line".to_string())?;

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

        doc.set_translation(
            "<block id=\"0\">\nKept\n</block>\n<block id=\"1\">\nIgnored\n</block>".to_string(),
        )?;

        assert_eq!(doc.text_blocks[0].translation.as_deref(), Some("Kept"));

        Ok(())
    }

    #[test]
    fn document_translation_pads_missing_tagged_blocks() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.set_translation("<block id=\"0\">\nOnly first\n</block>".to_string())?;

        assert_eq!(
            doc.text_blocks[0].translation.as_deref(),
            Some("Only first")
        );
        assert_eq!(doc.text_blocks[1].translation.as_deref(), Some(""));

        Ok(())
    }

    #[test]
    fn default_generate_options_are_strict() {
        let opts = GenerateOptions::default();
        assert_eq!(opts.temperature, 0.0);
        assert_eq!(opts.repeat_penalty, 1.0);
    }

    #[test]
    fn text_block_translation_strips_wrapping_quotes() -> anyhow::Result<()> {
        let mut block = TextBlock::default();
        block.set_translation("“quoted”".to_string())?;
        assert_eq!(block.translation.as_deref(), Some("quoted"));
        Ok(())
    }

    #[test]
    fn text_block_translation_keeps_japanese_dialogue_quotes() -> anyhow::Result<()> {
        let mut block = TextBlock::default();
        block.set_translation("「quoted」".to_string())?;
        assert_eq!(block.translation.as_deref(), Some("「quoted」"));
        Ok(())
    }
}
