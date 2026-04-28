use anyhow::Result;
use koharu_core::{NodeId, PageId};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationXmlEntry {
    pub page: PageId,
    pub node: NodeId,
    pub text: String,
}

pub fn export_translation_xml(texts: &[(PageId, NodeId, String)]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<koharu-translations version=\"1.0\">\n");
    for (page, node, text) in texts.iter() {
        out.push_str(&format!(
            "  <p page=\"{}\" node=\"{}\">{}</p>\n",
            page,
            node,
            escape_xml(text)
        ));
    }
    out.push_str("</koharu-translations>\n");
    out
}

pub fn parse_translation_xml(xml: &str) -> Result<Vec<TranslationXmlEntry>> {
    let mut entries = Vec::new();
    let mut cursor = 0;

    // If the file does not have the root tag, we could reject it or just skip.
    // Given the request to break backwards compatibility if needed, we proceed by parsing `page` and `node`.

    while let Some(open_rel) = xml[cursor..].find("<p") {
        let open = cursor + open_rel;
        let tag_end = xml[open..]
            .find('>')
            .map(|offset| open + offset)
            .ok_or_else(|| anyhow::anyhow!("unterminated <p> tag"))?;
        let tag = &xml[open..=tag_end];
        
        let page = parse_uuid_attr(tag, "page")?;
        let node = parse_uuid_attr(tag, "node")?;

        let content_start = tag_end + 1;
        let content_end = xml[content_start..]
            .find("</p>")
            .map(|offset| content_start + offset)
            .ok_or_else(|| anyhow::anyhow!("missing </p> tag for node {node}"))?;

        entries.push(TranslationXmlEntry {
            page: PageId(page),
            node: NodeId(node),
            text: unescape_xml(&xml[content_start..content_end])?,
        });
        cursor = content_end + "</p>".len();
    }

    Ok(entries)
}

fn parse_uuid_attr(tag: &str, attr: &str) -> Result<uuid::Uuid> {
    let prefix = format!("{attr}=\"");
    let attr_start = tag
        .find(&prefix)
        .map(|offset| offset + prefix.len())
        .ok_or_else(|| anyhow::anyhow!("translation paragraph missing {attr}"))?;
    let attr_end = tag[attr_start..]
        .find('"')
        .map(|offset| attr_start + offset)
        .ok_or_else(|| anyhow::anyhow!("unterminated {attr} attribute"))?;
    let val = &tag[attr_start..attr_end];
    uuid::Uuid::from_str(val).map_err(|_| anyhow::anyhow!("invalid uuid for {attr}: {val}"))
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
        let page = PageId::from(uuid::Uuid::from_str("00000000-0000-0000-0000-000000000001").unwrap());
        let node1 = NodeId::from(uuid::Uuid::from_str("00000000-0000-0000-0000-000000000002").unwrap());
        let node2 = NodeId::from(uuid::Uuid::from_str("00000000-0000-0000-0000-000000000003").unwrap());
        
        let xml = export_translation_xml(&[
            (page, node1, "A&B".to_string()), 
            (page, node2, "<hello>".to_string())
        ]);

        assert_eq!(
            xml,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<koharu-translations version=\"1.0\">\n  <p page=\"00000000-0000-0000-0000-000000000001\" node=\"00000000-0000-0000-0000-000000000002\">A&amp;B</p>\n  <p page=\"00000000-0000-0000-0000-000000000001\" node=\"00000000-0000-0000-0000-000000000003\">&lt;hello&gt;</p>\n</koharu-translations>\n"
        );
    }

    #[test]
    fn import_parses_unescaped_text_by_id() -> Result<()> {
        let page = PageId::from(uuid::Uuid::from_str("00000000-0000-0000-0000-000000000001").unwrap());
        let node1 = NodeId::from(uuid::Uuid::from_str("00000000-0000-0000-0000-000000000002").unwrap());
        let node2 = NodeId::from(uuid::Uuid::from_str("00000000-0000-0000-0000-000000000003").unwrap());

        let parsed = parse_translation_xml(
            "<koharu-translations version=\"1.0\">\n  <p page=\"00000000-0000-0000-0000-000000000001\" node=\"00000000-0000-0000-0000-000000000003\">&lt;world&gt;</p>\n  <p page=\"00000000-0000-0000-0000-000000000001\" node=\"00000000-0000-0000-0000-000000000002\">A&amp;B</p>\n</koharu-translations>",
        )?;

        assert_eq!(
            parsed,
            vec![
                TranslationXmlEntry {
                    page,
                    node: node2,
                    text: "<world>".to_string(),
                },
                TranslationXmlEntry {
                    page,
                    node: node1,
                    text: "A&B".to_string(),
                },
            ]
        );
        Ok(())
    }
}

