use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationXmlEntry {
    pub id: usize,
    pub text: String,
}

pub fn export_translation_xml(texts: &[String]) -> String {
    let mut out = String::from("<translations>\n");
    for (index, text) in texts.iter().enumerate() {
        out.push_str(&format!(
            "  <p id=\"{}\">{}</p>\n",
            index + 1,
            escape_xml(text)
        ));
    }
    out.push_str("</translations>\n");
    out
}

pub fn parse_translation_xml(xml: &str) -> Result<Vec<TranslationXmlEntry>> {
    let mut entries = Vec::new();
    let mut cursor = 0;

    while let Some(open_rel) = xml[cursor..].find("<p") {
        let open = cursor + open_rel;
        let tag_end = xml[open..]
            .find('>')
            .map(|offset| open + offset)
            .ok_or_else(|| anyhow::anyhow!("unterminated <p> tag"))?;
        let tag = &xml[open..=tag_end];
        let id = parse_id_attr(tag)?;
        let content_start = tag_end + 1;
        let content_end = xml[content_start..]
            .find("</p>")
            .map(|offset| content_start + offset)
            .ok_or_else(|| anyhow::anyhow!("missing </p> tag for id {id}"))?;

        entries.push(TranslationXmlEntry {
            id,
            text: unescape_xml(&xml[content_start..content_end])?,
        });
        cursor = content_end + "</p>".len();
    }

    Ok(entries)
}

fn parse_id_attr(tag: &str) -> Result<usize> {
    let id_start = tag
        .find("id=\"")
        .map(|offset| offset + "id=\"".len())
        .ok_or_else(|| anyhow::anyhow!("translation paragraph missing id"))?;
    let id_end = tag[id_start..]
        .find('"')
        .map(|offset| id_start + offset)
        .ok_or_else(|| anyhow::anyhow!("unterminated id attribute"))?;
    let id = tag[id_start..id_end]
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("invalid translation id"))?;
    if id == 0 {
        anyhow::bail!("translation id must be >= 1");
    }
    Ok(id)
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
        let xml = export_translation_xml(&["A&B".to_string(), "<hello>".to_string()]);

        assert_eq!(
            xml,
            "<translations>\n  <p id=\"1\">A&amp;B</p>\n  <p id=\"2\">&lt;hello&gt;</p>\n</translations>\n"
        );
    }

    #[test]
    fn import_parses_unescaped_text_by_id() -> Result<()> {
        let parsed = parse_translation_xml(
            "<translations><p id=\"2\">&lt;world&gt;</p><p id=\"1\">A&amp;B</p></translations>",
        )?;

        assert_eq!(
            parsed,
            vec![
                TranslationXmlEntry {
                    id: 2,
                    text: "<world>".to_string(),
                },
                TranslationXmlEntry {
                    id: 1,
                    text: "A&B".to_string(),
                },
            ]
        );
        Ok(())
    }
}
