use anyhow::Result;
use koharu_core::{Document, TextBlock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BlockStartTag {
    offset: usize,
    len: usize,
    id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BlockEndTag {
    offset: usize,
    len: usize,
}

pub(crate) trait TranslationTarget {
    fn translation_source(&self) -> Result<String>;
    fn apply_translation(&mut self, translation: String) -> Result<()>;
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

fn parse_tagged_blocks(translation: &str, expected_blocks: usize) -> Result<Option<Vec<String>>> {
    if find_next_block_start_tag(translation).is_none() {
        return Ok(None);
    }

    let mut blocks = vec![String::new(); expected_blocks];
    let mut cursor = translation;
    let mut found_any = false;
    let mut parsed_count = 0usize;
    let mut ignored_count = 0usize;

    while let Some(start_tag) = find_next_block_start_tag(cursor) {
        found_any = true;
        cursor = &cursor[start_tag.offset + start_tag.len..];

        let id = start_tag.id;
        if id >= expected_blocks {
            ignored_count += 1;
            tracing::warn!("Ignoring translated block id {id} for {expected_blocks} source blocks");
            let closing_tag = find_next_block_end_tag(cursor);
            let boundary = block_boundary(cursor, closing_tag.map(|tag| tag.offset));
            cursor = if closing_tag.map(|tag| tag.offset) == Some(boundary) {
                let closing_len = closing_tag.map(|tag| tag.len).unwrap_or(0);
                &cursor[boundary + closing_len..]
            } else {
                &cursor[boundary..]
            };
            continue;
        }

        let closing_tag = find_next_block_end_tag(cursor);
        let block_end = block_boundary(cursor, closing_tag.map(|tag| tag.offset));
        let content = unescape_block_text(cursor[..block_end].trim());

        if blocks[id].is_empty() {
            parsed_count += 1;
        } else {
            tracing::warn!("Translated block id {id} appeared more than once, keeping latest");
        }
        blocks[id] = content;

        cursor = if closing_tag.map(|tag| tag.offset) == Some(block_end) {
            let closing_len = closing_tag.map(|tag| tag.len).unwrap_or(0);
            &cursor[block_end + closing_len..]
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

fn split_legacy_lines(translation: &str, expected_blocks: usize) -> Vec<String> {
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

    translations
}

fn block_boundary(cursor: &str, closing_tag: Option<usize>) -> usize {
    let next_block_start = find_next_block_start_tag(cursor).map(|tag| tag.offset);
    match (closing_tag, next_block_start) {
        (Some(close), Some(next)) => close.min(next),
        (Some(close), None) => close,
        (None, Some(next)) => next,
        (None, None) => cursor.len(),
    }
}

fn find_next_block_start_tag(text: &str) -> Option<BlockStartTag> {
    let mut search_from = 0usize;
    while let Some(rel_start) = text[search_from..].find('<') {
        let offset = search_from + rel_start;
        if let Some((len, id)) = parse_block_start_tag(&text[offset..]) {
            return Some(BlockStartTag { offset, len, id });
        }
        search_from = offset + 1;
    }
    None
}

fn parse_block_start_tag(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    if bytes.first().copied()? != b'<' {
        return None;
    }

    let mut index = 1usize;
    skip_ascii_whitespace(bytes, &mut index);
    if !consume_ascii_keyword(bytes, &mut index, "block") {
        return None;
    }

    let mut parsed_id = None;
    loop {
        skip_ascii_whitespace(bytes, &mut index);
        match bytes.get(index).copied()? {
            b'>' => return parsed_id.map(|id| (index + 1, id)),
            b'/' if bytes.get(index + 1).copied() == Some(b'>') => {
                return parsed_id.map(|id| (index + 2, id));
            }
            _ => {}
        }

        let name_start = index;
        while matches!(
            bytes.get(index).copied(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-')
        ) {
            index += 1;
        }
        if index == name_start {
            return None;
        }
        let attr_name = &text[name_start..index];

        skip_ascii_whitespace(bytes, &mut index);
        if bytes.get(index).copied()? != b'=' {
            return None;
        }
        index += 1;
        skip_ascii_whitespace(bytes, &mut index);

        let attr_value = match bytes.get(index).copied()? {
            b'"' | b'\'' => {
                let quote = bytes[index];
                index += 1;
                let value_start = index;
                while bytes.get(index).copied()? != quote {
                    index += 1;
                }
                let value = &text[value_start..index];
                index += 1;
                value
            }
            _ => {
                let value_start = index;
                while matches!(bytes.get(index).copied(), Some(byte) if !byte.is_ascii_whitespace() && byte != b'>')
                {
                    index += 1;
                }
                &text[value_start..index]
            }
        };

        if attr_name.eq_ignore_ascii_case("id") {
            parsed_id = attr_value.parse::<usize>().ok();
        }
    }
}

fn find_next_block_end_tag(text: &str) -> Option<BlockEndTag> {
    let mut search_from = 0usize;
    while let Some(rel_start) = text[search_from..].find('<') {
        let offset = search_from + rel_start;
        if let Some(len) = parse_block_end_tag(&text[offset..]) {
            return Some(BlockEndTag { offset, len });
        }
        search_from = offset + 1;
    }
    None
}

fn parse_block_end_tag(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.first().copied()? != b'<' {
        return None;
    }

    let mut index = 1usize;
    skip_ascii_whitespace(bytes, &mut index);
    if bytes.get(index).copied()? != b'/' {
        return None;
    }
    index += 1;
    skip_ascii_whitespace(bytes, &mut index);
    if !consume_ascii_keyword(bytes, &mut index, "block") {
        return None;
    }
    skip_ascii_whitespace(bytes, &mut index);
    if bytes.get(index).copied()? != b'>' {
        return None;
    }
    Some(index + 1)
}

fn skip_ascii_whitespace(bytes: &[u8], index: &mut usize) {
    while matches!(bytes.get(*index).copied(), Some(byte) if byte.is_ascii_whitespace()) {
        *index += 1;
    }
}

fn consume_ascii_keyword(bytes: &[u8], index: &mut usize, keyword: &str) -> bool {
    let end = *index + keyword.len();
    let Some(slice) = bytes.get(*index..end) else {
        return false;
    };
    if !slice.eq_ignore_ascii_case(keyword.as_bytes()) {
        return false;
    }
    *index = end;
    true
}

impl TranslationTarget for Document {
    fn translation_source(&self) -> Result<String> {
        Ok(format_document_blocks(&self.text_blocks))
    }

    fn apply_translation(&mut self, translation: String) -> Result<()> {
        let translations = match parse_tagged_blocks(&translation, self.text_blocks.len())? {
            Some(blocks) => blocks,
            None => split_legacy_lines(&translation, self.text_blocks.len()),
        };

        for (block, translation) in self.text_blocks.iter_mut().zip(translations) {
            block.translation = Some(strip_wrapping_quotes(&translation));
        }
        Ok(())
    }
}

impl TranslationTarget for TextBlock {
    fn translation_source(&self) -> Result<String> {
        let source = self
            .text
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No source text found"))?;
        Ok(format!(
            r#"<block id="0">
{}
</block>"#,
            escape_block_text(&source)
        ))
    }

    fn apply_translation(&mut self, translation: String) -> Result<()> {
        let translation = match parse_tagged_blocks(&translation, 1)? {
            Some(blocks) => blocks.into_iter().next().unwrap_or_default(),
            None => translation,
        };
        self.translation = Some(strip_wrapping_quotes(&translation));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use koharu_core::{Document, TextBlock};

    use super::TranslationTarget;

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

        let source = doc.translation_source()?;
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

        doc.apply_translation(
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

        doc.apply_translation(
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

        doc.apply_translation("only one line".to_string())?;

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

        doc.apply_translation(
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

        doc.apply_translation("<block id=\"0\">\nFinal line".to_string())?;

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

        doc.apply_translation(
            "<block id=\"0\">\nKept\n</block>\n<block id=\"1\">\nIgnored\n</block>".to_string(),
        )?;

        assert_eq!(doc.text_blocks[0].translation.as_deref(), Some("Kept"));

        Ok(())
    }

    #[test]
    fn document_translation_accepts_relaxed_block_tag_formatting() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.apply_translation(
            "<block id = '1' >\nSecond\n</ block>\n<Block id=0>\nFirst\n</BLOCK>".to_string(),
        )?;

        assert_eq!(doc.text_blocks[0].translation.as_deref(), Some("First"));
        assert_eq!(doc.text_blocks[1].translation.as_deref(), Some("Second"));

        Ok(())
    }

    #[test]
    fn document_translation_accepts_unquoted_block_ids() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default()],
            ..Default::default()
        };

        doc.apply_translation("<block id=0>\nOnly first\n</block>".to_string())?;

        assert_eq!(
            doc.text_blocks[0].translation.as_deref(),
            Some("Only first")
        );

        Ok(())
    }

    #[test]
    fn document_translation_pads_missing_tagged_blocks() -> anyhow::Result<()> {
        let mut doc = Document {
            text_blocks: vec![TextBlock::default(), TextBlock::default()],
            ..Default::default()
        };

        doc.apply_translation("<block id=\"0\">\nOnly first\n</block>".to_string())?;

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
        block.apply_translation("“quoted”".to_string())?;
        assert_eq!(block.translation.as_deref(), Some("quoted"));
        Ok(())
    }

    #[test]
    fn text_block_source_uses_single_tagged_block() -> anyhow::Result<()> {
        let block = TextBlock {
            text: Some("1 < 2\nA & B".to_string()),
            ..Default::default()
        };

        let source = block.translation_source()?;
        assert_eq!(source, "<block id=\"0\">\n1 &lt; 2\nA &amp; B\n</block>");

        Ok(())
    }

    #[test]
    fn text_block_translation_extracts_tagged_block_content() -> anyhow::Result<()> {
        let mut block = TextBlock::default();
        block.apply_translation(
            "Sure.\n<block id=\"0\">\nTranslated &lt;line&gt;\n</block>\nDone.".to_string(),
        )?;
        assert_eq!(block.translation.as_deref(), Some("Translated <line>"));
        Ok(())
    }

    #[test]
    fn text_block_translation_keeps_multiline_plain_text() -> anyhow::Result<()> {
        let mut block = TextBlock::default();
        block.apply_translation("First line\nSecond line".to_string())?;
        assert_eq!(
            block.translation.as_deref(),
            Some("First line\nSecond line")
        );
        Ok(())
    }

    #[test]
    fn text_block_translation_keeps_japanese_dialogue_quotes() -> anyhow::Result<()> {
        let mut block = TextBlock::default();
        block.apply_translation("「quoted」".to_string())?;
        assert_eq!(block.translation.as_deref(), Some("「quoted」"));
        Ok(())
    }
}
