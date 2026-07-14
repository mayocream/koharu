use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{Language, TranslationContext, TranslationRequest, local::translation_system_prompt};

pub(super) fn prompts(request: &TranslationRequest) -> anyhow::Result<(String, String)> {
    let user = serde_json::to_string(&TranslationInput {
        source_language: request.source_language,
        target_language: request.target_language,
        context: &request.context,
        segments: &request.segments,
    })
    .context("failed to serialize translation input")?;
    Ok((translation_system_prompt(request), user))
}

pub(super) fn translations(provider: &str, text: &str) -> anyhow::Result<Vec<String>> {
    let text = text.trim();
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```JSON"))
        .or_else(|| text.strip_prefix("```"))
        .unwrap_or(text)
        .trim();
    let text = text.strip_suffix("```").unwrap_or(text).trim();
    serde_json::from_str::<TranslationOutput>(text)
        .with_context(|| format!("{provider} returned invalid translation JSON"))
        .map(|output| output.translations)
}

#[derive(Serialize)]
struct TranslationInput<'a> {
    source_language: Option<Language>,
    target_language: Language,
    context: &'a [TranslationContext],
    segments: &'a [String],
}

#[derive(Deserialize)]
struct TranslationOutput {
    translations: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_json_and_markdown_fences() {
        let expected = vec!["hello".to_owned(), "world".to_owned()];
        assert_eq!(
            translations("test", r#"{"translations":["hello","world"]}"#).unwrap(),
            expected
        );
        assert_eq!(
            translations(
                "test",
                "```json\n{\"translations\":[\"hello\",\"world\"]}\n```"
            )
            .unwrap(),
            expected
        );
    }

    #[test]
    fn prompt_payload_contains_ordered_context() {
        let request = TranslationRequest::new(["new"], Language::English)
            .with_context([TranslationContext::new("old", "previous")]);
        let (_, user) = prompts(&request).unwrap();
        assert!(user.contains(r#""context":[{"source":"old","translation":"previous"}]"#));
    }
}
