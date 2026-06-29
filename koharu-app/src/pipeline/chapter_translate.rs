//! Chapter-context translation helpers.
//!
//! Pure functions for stable block IDs, token budgeting, chunking, and
//! strict response validation. Wired into the pipeline driver for chapter mode.

use std::collections::HashSet;

use anyhow::{Result, bail};
use koharu_core::{
    NodeDataPatch, NodeId, NodePatch, Op, PageId, Scene, TextData, TextDataPatch,
};

use crate::llm;
use crate::pipeline::engines::support::text_nodes;
/// Default token budget per LLM request when chunking a chapter.
pub const DEFAULT_CHUNK_MAX_TOKENS: usize = 4_096;
/// Default maximum blocks per chunk when chunking a chapter.
pub const DEFAULT_CHUNK_MAX_BLOCKS: usize = 100;
pub const MIN_CHUNK_MAX_TOKENS: usize = 256;
pub const MAX_CHUNK_MAX_TOKENS: usize = 8_192;
pub const MIN_CHUNK_MAX_BLOCKS: usize = 1;
pub const MAX_CHUNK_MAX_BLOCKS: usize = 200;

/// Resolved chapter chunking limits after clamping optional overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChapterChunkConfig {
    pub max_tokens: usize,
    pub max_blocks: usize,
}

impl Default for ChapterChunkConfig {
    fn default() -> Self {
        Self {
            max_tokens: DEFAULT_CHUNK_MAX_TOKENS,
            max_blocks: DEFAULT_CHUNK_MAX_BLOCKS,
        }
    }
}

impl ChapterChunkConfig {
    pub fn resolve(token_budget: Option<u32>, max_blocks: Option<u32>) -> Self {
        Self {
            max_tokens: clamp_token_budget(token_budget),
            max_blocks: clamp_max_blocks(max_blocks),
        }
    }
}

pub fn clamp_token_budget(value: Option<u32>) -> usize {
    match value {
        Some(v) if (MIN_CHUNK_MAX_TOKENS as u32..=MAX_CHUNK_MAX_TOKENS as u32).contains(&v) => {
            v as usize
        }
        _ => DEFAULT_CHUNK_MAX_TOKENS,
    }
}

pub fn clamp_max_blocks(value: Option<u32>) -> usize {
    match value {
        Some(v) if (MIN_CHUNK_MAX_BLOCKS as u32..=MAX_CHUNK_MAX_BLOCKS as u32).contains(&v) => {
            v as usize
        }
        _ => DEFAULT_CHUNK_MAX_BLOCKS,
    }
}

/// Log chapter translation plan: pages, blocks, chunks, budget, and stable-id ranges.
pub fn log_chapter_translation_plan(
    page_count: usize,
    blocks: &[ChapterTranslationBlock],
    config: ChapterChunkConfig,
) {
    let chunks = chunk_blocks(blocks, config.max_tokens, config.max_blocks);
    let chunk_ranges: Vec<String> = chunks
        .iter()
        .filter_map(|chunk| {
            let first = chunk.first()?;
            let last = chunk.last()?;
            Some(if first.stable_id == last.stable_id {
                first.stable_id.clone()
            } else {
                format!("{}..={}", first.stable_id, last.stable_id)
            })
        })
        .collect();

    tracing::info!(
        page_count,
        block_count = blocks.len(),
        chunk_count = chunks.len(),
        token_budget = config.max_tokens,
        max_blocks = config.max_blocks,
        chunk_ranges = %chunk_ranges.join(", "),
        "chapter context translation starting"
    );
}

/// One OCR text block collected across pages for chapter translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterTranslationBlock {
    pub page_id: PageId,
    pub node_id: NodeId,
    pub page_number: u32,
    pub block_number: u32,
    pub stable_id: String,
    pub source_text: String,
}

/// Build a stable block tag id such as `P001-B002`.
pub fn stable_id(page_number: u32, block_number: u32) -> String {
    format!("P{page_number:03}-B{block_number:03}")
}

/// Rough token estimate for chunk budgeting.
///
/// ASCII characters count as one unit; other scripts (typical manga OCR) count
/// as two. The result is always at least one.
pub fn estimate_tokens(text: &str) -> usize {
    let units: usize = text
        .chars()
        .map(|ch| if ch.is_ascii() { 1 } else { 2 })
        .sum();
    units.max(1)
}

/// Estimated prompt cost of one tagged block line: `[id]text\n`.
pub fn block_token_cost(block: &ChapterTranslationBlock) -> usize {
    estimate_tokens(&block.stable_id) + 2 + estimate_tokens(&block.source_text) + 1
}

/// Split blocks into chunks whose estimated token cost does not exceed
/// `max_tokens` and whose block count does not exceed `max_blocks`.
/// Block order is preserved; chunks never split a block.
pub fn chunk_blocks(
    blocks: &[ChapterTranslationBlock],
    max_tokens: usize,
    max_blocks: usize,
) -> Vec<Vec<ChapterTranslationBlock>> {
    if blocks.is_empty() {
        return Vec::new();
    }
    let budget = max_tokens.max(1);
    let block_limit = max_blocks.max(1);
    let mut chunks: Vec<Vec<ChapterTranslationBlock>> = Vec::new();
    let mut current = Vec::new();
    let mut current_cost = 0usize;

    for block in blocks {
        let cost = block_token_cost(block);
        if !current.is_empty()
            && (current_cost + cost > budget || current.len() >= block_limit)
        {
            chunks.push(current);
            current = Vec::new();
            current_cost = 0;
        }
        current_cost += cost;
        current.push(block.clone());
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Split blocks into chunks whose estimated token cost does not exceed
/// `max_tokens`. Block order is preserved; chunks never split a block.
pub fn chunk_by_tokens(
    blocks: &[ChapterTranslationBlock],
    max_tokens: usize,
) -> Vec<Vec<ChapterTranslationBlock>> {
    chunk_blocks(blocks, max_tokens, usize::MAX)
}

/// Format tagged chapter blocks for the LLM user message.
pub fn format_chapter_blocks(blocks: &[ChapterTranslationBlock]) -> String {
    let mut lines = Vec::new();
    let mut current_page = None;
    for block in blocks {
        if current_page != Some(block.page_number) {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            current_page = Some(block.page_number);
            lines.push(format!("Page {}:", block.page_number));
        }
        lines.push(format!("[{}]{}", block.stable_id, block.source_text));
    }
    lines.join("\n")
}

/// Parse `[P001-B001] translated text` pairs from an LLM response.
pub fn parse_stable_id_blocks(response: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut cursor = response;
    while let Some((offset, tag_len, id)) = find_next_stable_id_tag(cursor) {
        cursor = &cursor[offset + tag_len..];
        let content_end = find_next_stable_id_tag(cursor)
            .map(|(next_offset, _, _)| next_offset)
            .unwrap_or(cursor.len());
        let content = cursor[..content_end].trim().to_string();
        out.push((id, content));
        cursor = &cursor[content_end..];
    }
    Ok(out)
}

/// Validate parsed output against the expected stable ids and source blocks.
///
/// Requires an exact one-to-one match in the same order with no duplicates.
/// Rejects translations that look like runaway repetition.
pub fn validate_chapter_translations(
    blocks: &[ChapterTranslationBlock],
    parsed: &[(String, String)],
) -> Result<Vec<String>> {
    let expected_ids: Vec<String> = blocks.iter().map(|b| b.stable_id.clone()).collect();
    let translations = validate_response(&expected_ids, parsed)?;
    for (block, translation) in blocks.iter().zip(translations.iter()) {
        if is_repetitive_translation(&block.source_text, translation) {
            bail!(
                "chapter translation: repetitive output for block '{}'",
                block.stable_id
            );
        }
    }
    Ok(translations)
}

/// True when a translation is extremely long compared to its source text.
pub fn is_repetitive_translation(source: &str, translation: &str) -> bool {
    let threshold = 300.max(source.len().saturating_mul(8));
    translation.len() > threshold
}

/// Validate parsed output against the expected stable ids.
///
/// Requires an exact one-to-one match in the same order with no duplicates.
/// Returns translations aligned with `expected_ids`.
pub fn validate_response(
    expected_ids: &[String],
    parsed: &[(String, String)],
) -> Result<Vec<String>> {
    if parsed.len() != expected_ids.len() {
        bail!(
            "chapter translation: expected {} block(s), got {}",
            expected_ids.len(),
            parsed.len()
        );
    }

    let mut seen = HashSet::with_capacity(expected_ids.len());
    let mut translations = Vec::with_capacity(expected_ids.len());

    for (index, (id, text)) in parsed.iter().enumerate() {
        if !is_stable_id(id) {
            bail!("chapter translation: invalid block id '{id}' at index {index}");
        }
        if id != &expected_ids[index] {
            bail!(
                "chapter translation: block order mismatch at index {index}: expected '{}', got '{id}'",
                expected_ids[index]
            );
        }
        if !seen.insert(id.clone()) {
            bail!("chapter translation: duplicate block id '{id}'");
        }
        translations.push(text.clone());
    }

    Ok(translations)
}

/// Collect translatable OCR blocks across `page_ids` in run order.
pub fn collect_blocks(
    scene: &Scene,
    page_ids: &[PageId],
    skip_pages: &HashSet<PageId>,
) -> Vec<ChapterTranslationBlock> {
    let mut blocks = Vec::new();
    for (page_index, page_id) in page_ids.iter().enumerate() {
        if skip_pages.contains(page_id) {
            continue;
        }
        let page_number = (page_index + 1) as u32;
        let mut block_number = 0u32;
        for (node_id, _, text_data) in text_nodes(scene, *page_id) {
            if !should_translate(node_id, text_data) {
                continue;
            }
            let source = text_data.text.as_ref().expect("filtered by should_translate");
            block_number += 1;
            blocks.push(ChapterTranslationBlock {
                page_id: *page_id,
                node_id,
                page_number,
                block_number,
                stable_id: stable_id(page_number, block_number),
                source_text: source.clone(),
            });
        }
    }
    blocks
}

/// Build `UpdateNode` ops that write translations back to text nodes.
pub fn blocks_to_ops(blocks: &[ChapterTranslationBlock], translations: &[String]) -> Vec<Op> {
    blocks
        .iter()
        .zip(translations)
        .map(|(block, translation)| Op::UpdateNode {
            page: block.page_id,
            id: block.node_id,
            patch: NodePatch {
                data: Some(NodeDataPatch::Text(TextDataPatch {
                    translation: Some(Some(translation.clone())),
                    ..Default::default()
                })),
                transform: None,
                visible: None,
            },
            prev: NodePatch::default(),
        })
        .collect()
}

/// Translate blocks in token-budgeted chunks. Returns translations aligned with
/// `blocks`; fails without partial results if any chunk is invalid.
pub async fn translate_blocks_chunked(
    llm: &llm::Model,
    blocks: &[ChapterTranslationBlock],
    config: ChapterChunkConfig,
    target_language: Option<&str>,
    system_prompt: Option<&str>,
) -> Result<Vec<String>> {
    if blocks.is_empty() {
        return Ok(Vec::new());
    }
    let chunks = chunk_blocks(blocks, config.max_tokens, config.max_blocks);
    let mut translations = Vec::with_capacity(blocks.len());
    for chunk in chunks {
        let chunk_translations = llm
            .translate_chapter_blocks(&chunk, target_language, system_prompt)
            .await?;
        translations.extend(chunk_translations);
    }
    Ok(translations)
}

fn should_translate(_id: NodeId, text_data: &TextData) -> bool {
    text_data
        .text
        .as_ref()
        .is_some_and(|source| !source.trim().is_empty())
}

fn is_stable_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 9
        && bytes[0] == b'P'
        && bytes[4] == b'-'
        && bytes[5] == b'B'
        && bytes[1..4].iter().all(u8::is_ascii_digit)
        && bytes[6..9].iter().all(u8::is_ascii_digit)
}

fn parse_stable_id_at(text: &str) -> Option<(usize, String)> {
    if !text.starts_with('[') {
        return None;
    }
    let rest = &text[1..];
    let end = rest.find(']')?;
    let id = &rest[..end];
    if !is_stable_id(id) {
        return None;
    }
    Some((1 + end + 1, id.to_string()))
}

fn find_next_stable_id_tag(text: &str) -> Option<(usize, usize, String)> {
    let mut line_start = 0;
    while line_start <= text.len() {
        let line = &text[line_start..];
        let indent = line
            .as_bytes()
            .iter()
            .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
            .count();
        let offset = line_start + indent;
        if let Some((len, id)) = parse_stable_id_at(&text[offset..]) {
            return Some((offset, len, id));
        }
        let Some(next_newline) = line.find('\n') else {
            break;
        };
        line_start += next_newline + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn block(page: u32, block_num: u32, text: &str) -> ChapterTranslationBlock {
        ChapterTranslationBlock {
            page_id: PageId(Uuid::from_u128(page as u128)),
            node_id: NodeId(Uuid::from_u128((page * 100 + block_num) as u128)),
            page_number: page,
            block_number: block_num,
            stable_id: stable_id(page, block_num),
            source_text: text.to_string(),
        }
    }

    #[test]
    fn stable_id_formats_page_and_block_numbers() {
        assert_eq!(stable_id(1, 1), "P001-B001");
        assert_eq!(stable_id(10, 42), "P010-B042");
    }

    #[test]
    fn estimate_tokens_counts_ascii_and_cjk_differently() {
        assert_eq!(estimate_tokens(""), 1);
        assert_eq!(estimate_tokens("hello"), 5);
        assert_eq!(estimate_tokens("こんにちは"), 10);
    }

    #[test]
    fn block_token_cost_includes_tag_overhead() {
        let b = block(1, 1, "hi");
        assert!(block_token_cost(&b) > estimate_tokens("hi"));
    }

    #[test]
    fn chunk_by_tokens_preserves_order_and_respects_budget() {
        let blocks = vec![
            block(1, 1, "aaaa"),
            block(1, 2, "bbbb"),
            block(2, 1, "cccc"),
        ];
        // Two blocks cost 16 tokens each; 32 fits a pair, 33 forces a third chunk.
        let chunks = chunk_by_tokens(&blocks, 32);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2);
        assert_eq!(chunks[1].len(), 1);
        assert_eq!(chunks[0][0].stable_id, "P001-B001");
        assert_eq!(chunks[1][0].stable_id, "P002-B001");
    }

    #[test]
    fn chunk_by_tokens_keeps_oversized_block_alone() {
        let long_text = "あ".repeat(500);
        let blocks = vec![block(1, 1, &long_text), block(1, 2, "x")];
        let chunks = chunk_by_tokens(&blocks, 10);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 1);
        assert_eq!(chunks[1].len(), 1);
    }

    #[test]
    fn format_chapter_blocks_groups_by_page() {
        let blocks = vec![
            block(1, 1, "first"),
            block(1, 2, "second"),
            block(2, 1, "third"),
        ];
        let body = format_chapter_blocks(&blocks);
        assert!(body.contains("Page 1:"));
        assert!(body.contains("[P001-B001]first"));
        assert!(body.contains("Page 2:"));
        assert!(body.contains("[P002-B001]third"));
    }

    #[test]
    fn parse_stable_id_blocks_extracts_tagged_lines() {
        let response = "Page 1:\n[P001-B001] hello\n[P001-B002] world\n";
        let parsed = parse_stable_id_blocks(response).unwrap();
        assert_eq!(
            parsed,
            vec![
                ("P001-B001".to_string(), "hello".to_string()),
                ("P001-B002".to_string(), "world".to_string()),
            ]
        );
    }

    #[test]
    fn parse_stable_id_blocks_ignores_numeric_tags() {
        let response = "[1]legacy\n[P001-B001] ok\n";
        let parsed = parse_stable_id_blocks(response).unwrap();
        assert_eq!(parsed, vec![("P001-B001".to_string(), "ok".to_string())]);
    }

    #[test]
    fn validate_response_accepts_exact_match() {
        let expected = vec!["P001-B001".to_string(), "P001-B002".to_string()];
        let parsed = vec![
            ("P001-B001".to_string(), "hi".to_string()),
            ("P001-B002".to_string(), "bye".to_string()),
        ];
        let out = validate_response(&expected, &parsed).unwrap();
        assert_eq!(out, vec!["hi", "bye"]);
    }

    #[test]
    fn validate_response_rejects_missing_block() {
        let expected = vec!["P001-B001".to_string(), "P001-B002".to_string()];
        let parsed = vec![("P001-B001".to_string(), "hi".to_string())];
        assert!(validate_response(&expected, &parsed).is_err());
    }

    #[test]
    fn validate_response_rejects_extra_block() {
        let expected = vec!["P001-B001".to_string()];
        let parsed = vec![
            ("P001-B001".to_string(), "hi".to_string()),
            ("P001-B002".to_string(), "extra".to_string()),
        ];
        assert!(validate_response(&expected, &parsed).is_err());
    }

    #[test]
    fn validate_response_rejects_reordered_blocks() {
        let expected = vec!["P001-B001".to_string(), "P001-B002".to_string()];
        let parsed = vec![
            ("P001-B002".to_string(), "second".to_string()),
            ("P001-B001".to_string(), "first".to_string()),
        ];
        assert!(validate_response(&expected, &parsed).is_err());
    }

    #[test]
    fn chunk_blocks_splits_on_block_count() {
        let blocks: Vec<_> = (1..=5)
            .map(|n| block(1, n, "x"))
            .collect();
        let chunks = chunk_blocks(&blocks, usize::MAX, 2);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 2);
        assert_eq!(chunks[1].len(), 2);
        assert_eq!(chunks[2].len(), 1);
    }

    #[test]
    fn chapter_chunk_config_clamps_invalid_values_to_defaults() {
        assert_eq!(
            ChapterChunkConfig::resolve(Some(10_000), Some(500)),
            ChapterChunkConfig {
                max_tokens: DEFAULT_CHUNK_MAX_TOKENS,
                max_blocks: DEFAULT_CHUNK_MAX_BLOCKS,
            }
        );
        assert_eq!(
            ChapterChunkConfig::resolve(Some(512), Some(50)),
            ChapterChunkConfig {
                max_tokens: 512,
                max_blocks: 50,
            }
        );
    }

    #[test]
    fn validate_chapter_translations_rejects_repetitive_output() {
        let blocks = vec![block(1, 1, "hi")];
        let long = "x".repeat(301);
        let parsed = vec![("P001-B001".to_string(), long)];
        assert!(validate_chapter_translations(&blocks, &parsed).is_err());
    }

    #[test]
    fn is_repetitive_translation_uses_source_scaled_threshold() {
        assert!(!is_repetitive_translation("hello", "hello world"));
        assert!(is_repetitive_translation("hi", &"x".repeat(301)));
        assert!(is_repetitive_translation("x".repeat(50).as_str(), &"y".repeat(401)));
    }

    #[test]
    fn validate_response_rejects_duplicate_ids() {
        let expected = vec!["P001-B001".to_string(), "P001-B002".to_string()];
        let parsed = vec![
            ("P001-B001".to_string(), "a".to_string()),
            ("P001-B001".to_string(), "b".to_string()),
        ];
        assert!(validate_response(&expected, &parsed).is_err());
    }
}
