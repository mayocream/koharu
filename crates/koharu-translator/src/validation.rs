use std::fmt::Write as _;

use anyhow::{Context as _, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Language;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TranslationValidationRule {
    pub name: String,
    pub pattern: String,
}

#[derive(Clone, Debug)]
pub struct TranslationValidator {
    rules: Vec<CompiledRule>,
}

#[derive(Clone, Debug)]
struct CompiledRule {
    name: String,
    pattern: String,
    expression: Regex,
}

impl TranslationValidator {
    pub fn new(rules: &[TranslationValidationRule]) -> Result<Self> {
        let rules = rules
            .iter()
            .map(|rule| {
                let name = rule.name.trim();
                if name.is_empty() {
                    bail!("translation validation rule names must not be empty");
                }
                if rule.pattern.is_empty() {
                    bail!("translation validation pattern for {name} must not be empty");
                }
                Ok(CompiledRule {
                    name: name.to_owned(),
                    pattern: rule.pattern.clone(),
                    expression: Regex::new(&rule.pattern).with_context(|| {
                        format!("invalid translation validation pattern for {name}")
                    })?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { rules })
    }

    #[must_use]
    pub fn feedback(&self, translations: &[String], target_language: Language) -> Option<String> {
        let mut violations = None;
        for rule in &self.rules {
            if translations
                .iter()
                .any(|translation| rule.expression.is_match(translation))
            {
                let violations = violations.get_or_insert_with(String::new);
                if !violations.is_empty() {
                    violations.push('\n');
                }
                write!(violations, "- {} (`{}`)", rule.name, rule.pattern)
                    .expect("writing to a String cannot fail");
            }
        }
        let violations = violations?;
        Some(format!(
            "Your previous response was invalid because one or more translated `text` fields matched forbidden validation rules:\n{violations}\n\nRegenerate the entire JSON response.\nTranslate every segment completely into {target_language}.\nNo listed forbidden pattern is allowed in any `text` field.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_forbidden_latin_script_in_translation_text() {
        let validator = TranslationValidator::new(&[TranslationValidationRule {
            name: "Latin-script letters".to_owned(),
            pattern: "[A-Za-z]".to_owned(),
        }])
        .unwrap();

        let feedback = validator
            .feedback(&["Если я'll make him...".to_owned()], Language::Russian)
            .unwrap();

        assert!(feedback.contains("Regenerate the entire JSON response"));
        assert!(feedback.contains("Translate every segment completely into Russian"));
        assert!(feedback.contains("- Latin-script letters (`[A-Za-z]`)"));
    }

    #[test]
    fn reports_every_violated_rule_in_one_retry() {
        let validator = TranslationValidator::new(&[
            TranslationValidationRule {
                name: "Latin-script letters".to_owned(),
                pattern: "[A-Za-z]".to_owned(),
            },
            TranslationValidationRule {
                name: "Japanese kana".to_owned(),
                pattern: r"[\p{Hiragana}\p{Katakana}]".to_owned(),
            },
        ])
        .unwrap();

        let feedback = validator
            .feedback(&["English と日本語".to_owned()], Language::Russian)
            .unwrap();

        assert!(feedback.contains("Latin-script letters"));
        assert!(feedback.contains("Japanese kana"));
    }

    #[test]
    fn rejects_invalid_regular_expression() {
        let error = TranslationValidator::new(&[TranslationValidationRule {
            name: "Broken".to_owned(),
            pattern: "[".to_owned(),
        }])
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("invalid translation validation pattern")
        );
    }
}
