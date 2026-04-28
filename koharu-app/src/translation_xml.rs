use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationXmlEntry {
    pub page_id: Option<String>,
    pub node_id: Option<String>,
    pub id: Option<usize>, // Legacy index-based ID
    pub text: String,
}

pub struct TranslationXmlExportItem {
    pub page_id: String,
    pub node_id: String,
    pub text: String,
}

pub fn export_translation_xml(items: &[TranslationXmlExportItem]) -> String {
    let mut out = String::from("<translations version=\"1.0\">\n");
    for item in items {
        out.push_str(&format!(
            "  <entry page=\"{}\" node=\"{}\">{}</entry>\n",
            item.page_id,
            item.node_id,
            escape_xml(&item.text)
        ));
    }
    out.push_str("</translations>\n");
    out
}

pub fn parse_translation_xml(xml: &str) -> Result<Vec<TranslationXmlEntry>> {
    let mut entries = Vec::new();
    let mut cursor = 0;

    // Try V1 format first (<entry page="..." node="...">)
    while let Some(open_rel) = xml[cursor..].find("<entry") {
        let open = cursor + open_rel;
        let tag_end = xml[open..]
            .find('>')
            .map(|offset| open + offset)
            .ok_or_else(|| anyhow::anyhow!("unterminated <entry> tag"))?;
        let tag = &xml[open..=tag_end];
        
        let page_id = parse_attr(tag, "page").ok();
        let node_id = parse_attr(tag, "node").ok();
        
        let content_start = tag_end + 1;
        let content_end = xml[content_start..]
            .find("</entry>")
            .map(|offset| content_start + offset)
            .ok_or_else(|| anyhow::anyhow!("missing </entry> tag"))?;

        entries.push(TranslationXmlEntry {
            page_id,
            node_id,
            id: None,
            text: unescape_xml(&xml[content_start..content_end])?,
        });
        cursor = content_end + "</entry>".len();
    }

    if !entries.is_empty() {
        return Ok(entries);
    }

    // Fallback to V0 legacy format (<p id="...">)
    cursor = 0;
    while let Some(open_rel) = xml[cursor..].find("<p") {
        let open = cursor + open_rel;
        let tag_end = xml[open..]
            .find('>')
            .map(|offset| open + offset)
            .ok_or_else(|| anyhow::anyhow!("unterminated <p> tag"))?;
        let tag = &xml[open..=tag_end];
        let id = parse_attr(tag, "id")?.parse::<usize>().ok();
        let content_start = tag_end + 1;
        let content_end = xml[content_start..]
            .find("</p>")
            .map(|offset| content_start + offset)
            .ok_or_else(|| anyhow::anyhow!("missing </p> tag"))?;

        entries.push(TranslationXmlEntry {
            page_id: None,
            node_id: None,
            id,
            text: unescape_xml(&xml[content_start..content_end])?,
        });
        cursor = content_end + "</p>".len();
    }

    Ok(entries)
}

fn parse_attr(tag: &str, name: &str) -> Result<String> {
    let key = format!("{}=\"", name);
    let start = tag
        .find(&key)
        .map(|offset| offset + key.len())
        .ok_or_else(|| anyhow::anyhow!("attribute {} missing in tag", name))?;
    let end = tag[start..]
        .find('"')
        .map(|offset| start + offset)
        .ok_or_else(|| anyhow::anyhow!("unterminated attribute {}", name))?;
    Ok(tag[start..end].to_string())
}


fn escape_xml(text: &str) -> String {
    text.chars()
        .flat_map(|ch| match ch {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect::<Vec<_>>(),
            '>' => "&gt;".chars().collect::<Vec<_>>(),
            '"' => "&quot;".chars().collect::<Vec<_>>(),
            '\'' => "&apos;".chars().collect::<Vec<_>>(),
            _ => vec![ch],
        })
        .collect()
}

fn unescape_xml(text: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let remaining = &text[cursor..];
        if let Some(stripped) = remaining.strip_prefix('&') {
            let Some(end) = stripped.find(';') else {
                anyhow::bail!("unterminated XML entity");
            };
            let entity = &stripped[..end];
            let decoded = match entity {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => anyhow::bail!("unsupported XML entity: &{entity};"),
            };
            out.push(decoded);
            cursor += entity.len() + 2;
        } else {
            let ch = remaining
                .chars()
                .next()
                .expect("cursor is always inside text");
            out.push(ch);
            cursor += ch.len_utf8();
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_escapes_translation_text() {
        let xml = export_translation_xml(&[
            TranslationXmlExportItem {
                page_id: "p1".into(),
                node_id: "n1".into(),
                text: "A&B".into(),
            },
            TranslationXmlExportItem {
                page_id: "p1".into(),
                node_id: "n2".into(),
                text: "<hello>".into(),
            },
        ]);

        assert!(xml.contains("version=\"1.0\""));
        assert!(xml.contains("<entry page=\"p1\" node=\"n1\">A&amp;B</entry>"));
        assert!(xml.contains("<entry page=\"p1\" node=\"n2\">&lt;hello&gt;</entry>"));
    }

    #[test]
    fn import_parses_unescaped_text_by_uuids() -> Result<()> {
        let parsed = parse_translation_xml(
            "<translations><entry page=\"p1\" node=\"n1\">&lt;world&gt;</entry></translations>",
        )?;

        assert_eq!(
            parsed,
            vec![TranslationXmlEntry {
                page_id: Some("p1".into()),
                node_id: Some("n1".into()),
                id: None,
                text: "<world>".into(),
            },]
        );
        Ok(())
    }

    #[test]
    fn import_parses_legacy_v0_format() -> Result<()> {
        let parsed = parse_translation_xml(
            "<translations><p id=\"2\">&lt;world&gt;</p><p id=\"1\">A&amp;B</p></translations>",
        )?;

        assert_eq!(
            parsed,
            vec![
                TranslationXmlEntry {
                    page_id: None,
                    node_id: None,
                    id: Some(2),
                    text: "<world>".into(),
                },
                TranslationXmlEntry {
                    page_id: None,
                    node_id: None,
                    id: Some(1),
                    text: "A&B".into(),
                },
            ]
        );
        Ok(())
    }
}
