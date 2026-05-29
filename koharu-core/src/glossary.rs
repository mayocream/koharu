//! Glossary types + prompt rendering.
//!
//! A glossary is a list of user-defined terms (character names, honorifics,
//! place names, sound effects) that should always be translated the same way.
//! Only the entries whose source term actually appears on the page are rendered
//! into the translation prompt, so large glossaries do not bloat every request.
//!
//! This module is pure data + string logic (no I/O), matching the rest of
//! `koharu-core`. The actual injection into the system prompt happens in the
//! `llm` translation engine.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A single glossary mapping from a source-language term to its required
/// translation, with optional translator guidance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryEntry {
    /// Term as it appears in the source text (e.g. a name or honorific).
    pub source: String,
    /// The translation that must be used for `source`.
    pub target: String,
    /// Optional note for the model: gender, "keep romanized", reading, etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// When `Some(false)` the entry is ignored. Absent / `Some(true)` is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

impl GlossaryEntry {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

const HEADING: &str = "Use the following glossary for consistent terminology. \
Translate each term on the left exactly as the value on the right, \
unless doing so is grammatically impossible:";

/// Render the glossary entries relevant to `source_text` into a prompt block.
///
/// An entry is relevant when it is enabled and its `source` occurs in
/// `source_text` (case-insensitive substring match, which suits CJK source
/// languages that have no word boundaries). Longer, more specific terms are
/// listed first. Returns `None` when nothing applies.
pub fn render_glossary_section(entries: &[GlossaryEntry], source_text: &str) -> Option<String> {
    let haystack = source_text.to_lowercase();

    let mut matched: Vec<&GlossaryEntry> = entries
        .iter()
        .filter(|entry| entry.is_enabled())
        .filter(|entry| !entry.source.trim().is_empty() && !entry.target.trim().is_empty())
        .filter(|entry| haystack.contains(&entry.source.to_lowercase()))
        .collect();

    if matched.is_empty() {
        return None;
    }

    // Longer source terms first so more specific matches take precedence in the
    // model's attention; stable tiebreak on the source for determinism.
    matched.sort_by(|a, b| {
        b.source
            .chars()
            .count()
            .cmp(&a.source.chars().count())
            .then_with(|| a.source.cmp(&b.source))
    });

    let mut out = String::from(HEADING);
    for entry in matched {
        out.push_str("\n- ");
        out.push_str(entry.source.trim());
        out.push_str(" => ");
        out.push_str(entry.target.trim());
        if let Some(note) = entry.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            out.push_str(" (");
            out.push_str(note);
            out.push(')');
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source: &str, target: &str) -> GlossaryEntry {
        GlossaryEntry {
            source: source.to_string(),
            target: target.to_string(),
            note: None,
            enabled: None,
        }
    }

    #[test]
    fn renders_only_matching_entries() {
        let entries = vec![entry("春日", "Kasuga"), entry("海", "sea")];
        let section = render_glossary_section(&entries, "おはよう春日先輩").unwrap();
        assert!(section.contains("春日 => Kasuga"));
        assert!(!section.contains("海 => sea"));
    }

    #[test]
    fn returns_none_when_nothing_matches() {
        let entries = vec![entry("海", "sea")];
        assert!(render_glossary_section(&entries, "こんにちは").is_none());
    }

    #[test]
    fn longer_terms_come_first() {
        let entries = vec![entry("春日", "Kasuga"), entry("春日先輩", "Kasuga-senpai")];
        let section = render_glossary_section(&entries, "やあ春日先輩").unwrap();
        let long = section.find("春日先輩 => Kasuga-senpai").unwrap();
        let short = section.find("春日 => Kasuga").unwrap();
        assert!(long < short, "longer term should be listed first");
    }

    #[test]
    fn disabled_entries_are_skipped() {
        let mut e = entry("先輩", "senpai");
        e.enabled = Some(false);
        assert!(render_glossary_section(&[e], "先輩").is_none());
    }

    #[test]
    fn notes_are_appended() {
        let mut e = entry("先輩", "senpai");
        e.note = Some("keep honorific".to_string());
        let section = render_glossary_section(&[e], "先輩").unwrap();
        assert!(section.contains("先輩 => senpai (keep honorific)"));
    }

    #[test]
    fn blank_target_is_ignored() {
        let entries = vec![entry("春日", "   ")];
        assert!(render_glossary_section(&entries, "春日").is_none());
    }
}
