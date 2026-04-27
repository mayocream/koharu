use std::fs;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::config::AppConfig;

const TERMINOLOGY_DIR: &str = "terminology";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyLibraryConfig {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyEntry {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyLibrary {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub terms: Vec<TerminologyEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyLibraryPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub priority: Option<i32>,
    pub terms: Option<Vec<TerminologyEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminologyLibraryRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportTerminologyCsvRequest {
    pub csv: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTerminologyLibrariesResponse {
    pub libraries: Vec<TerminologyLibrary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveGlossary {
    pub priority: i32,
    pub terms: Vec<TerminologyEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderReplacement {
    pub placeholder: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedText {
    pub text: String,
    pub replacements: Vec<PlaceholderReplacement>,
}

pub const PLACEHOLDER_SYSTEM_PROMPT: &str = "Strictly preserve all placeholders like {{1}}, {{2}}, etc. Do not translate or modify them. Ensure they remain in the translated sentence as anchors.";

pub fn terminology_dir(config: &AppConfig) -> Utf8PathBuf {
    config.data.path.join(TERMINOLOGY_DIR)
}

pub fn load_libraries(config: &AppConfig) -> Result<Vec<TerminologyLibrary>> {
    let dir = terminology_dir(config);
    ensure_dir(&dir)?;
    config
        .terminology_libraries
        .iter()
        .map(|meta| {
            let terms = read_csv_file(&dir.join(&meta.file))
                .with_context(|| format!("failed to read terminology library `{}`", meta.name))?;
            Ok(TerminologyLibrary {
                id: meta.id.clone(),
                name: meta.name.clone(),
                enabled: meta.enabled,
                priority: meta.priority,
                terms,
            })
        })
        .collect()
}

pub fn load_active_glossaries(config: &AppConfig) -> Result<Vec<ActiveGlossary>> {
    Ok(load_libraries(config)?
        .into_iter()
        .filter(|library| library.enabled && !library.terms.is_empty())
        .map(|library| ActiveGlossary {
            priority: library.priority,
            terms: library.terms,
        })
        .collect())
}

pub fn create_library(config: &mut AppConfig, name: &str) -> Result<TerminologyLibrary> {
    let id = Uuid::now_v7().to_string();
    let file = format!("{id}.csv");
    let trimmed = name.trim();
    let name = if trimmed.is_empty() {
        "Terminology".to_string()
    } else {
        trimmed.to_string()
    };
    let priority = next_priority(config);
    let meta = TerminologyLibraryConfig {
        id: id.clone(),
        name: name.clone(),
        enabled: true,
        priority,
        file: file.clone(),
    };
    let dir = terminology_dir(config);
    write_csv_file(&dir.join(&file), &[])?;
    config.terminology_libraries.push(meta);
    Ok(TerminologyLibrary {
        id,
        name,
        enabled: true,
        priority,
        terms: Vec::new(),
    })
}

pub fn update_library(
    config: &mut AppConfig,
    id: &str,
    patch: TerminologyLibraryPatch,
) -> Result<Option<TerminologyLibrary>> {
    let dir = terminology_dir(config);
    let Some(index) = config
        .terminology_libraries
        .iter()
        .position(|library| library.id == id)
    else {
        return Ok(None);
    };

    if let Some(name) = patch.name {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            config.terminology_libraries[index].name = trimmed.to_string();
        }
    }
    if let Some(enabled) = patch.enabled {
        config.terminology_libraries[index].enabled = enabled;
    }
    if let Some(priority) = patch.priority {
        config.terminology_libraries[index].priority = priority;
    }
    if let Some(terms) = patch.terms {
        let file = config.terminology_libraries[index].file.clone();
        write_csv_file(&dir.join(file), &terms)?;
    }

    let meta = &config.terminology_libraries[index];
    let terms = read_csv_file(&dir.join(&meta.file))?;
    Ok(Some(TerminologyLibrary {
        id: meta.id.clone(),
        name: meta.name.clone(),
        enabled: meta.enabled,
        priority: meta.priority,
        terms,
    }))
}

pub fn delete_library(config: &mut AppConfig, id: &str) -> Result<bool> {
    let Some(index) = config
        .terminology_libraries
        .iter()
        .position(|library| library.id == id)
    else {
        return Ok(false);
    };
    let meta = config.terminology_libraries.remove(index);
    let path = terminology_dir(config).join(meta.file);
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to delete `{path}`"))?;
    }
    Ok(true)
}

pub fn import_csv(
    config: &mut AppConfig,
    id: &str,
    csv: &str,
) -> Result<Option<TerminologyLibrary>> {
    let terms = parse_csv(csv.as_bytes())?;
    update_library(
        config,
        id,
        TerminologyLibraryPatch {
            name: None,
            enabled: None,
            priority: None,
            terms: Some(terms),
        },
    )
}

pub fn export_csv(config: &AppConfig, id: &str) -> Result<Option<String>> {
    let Some(meta) = config
        .terminology_libraries
        .iter()
        .find(|library| library.id == id)
    else {
        return Ok(None);
    };
    let terms = read_csv_file(&terminology_dir(config).join(&meta.file))?;
    Ok(Some(serialize_csv(&terms)?))
}

pub fn protect_text(source: &str, glossaries: &[ActiveGlossary]) -> ProtectedText {
    let candidates = sorted_terms(glossaries);
    if candidates.is_empty() {
        return ProtectedText {
            text: source.to_string(),
            replacements: Vec::new(),
        };
    }

    let mut text = String::with_capacity(source.len());
    let mut replacements = Vec::new();
    let mut cursor = 0;

    while cursor < source.len() {
        let remaining = &source[cursor..];
        if let Some(term) = candidates
            .iter()
            .find(|candidate| remaining.starts_with(candidate.source.as_str()))
        {
            let placeholder = format!("{{{{{}}}}}", replacements.len() + 1);
            text.push_str(&placeholder);
            replacements.push(PlaceholderReplacement {
                placeholder,
                target: term.target.clone(),
            });
            cursor += term.source.len();
            continue;
        }

        let ch = remaining
            .chars()
            .next()
            .expect("cursor is always inside source");
        text.push(ch);
        cursor += ch.len_utf8();
    }

    ProtectedText { text, replacements }
}

pub fn restore_text(source: &str, replacements: &[PlaceholderReplacement]) -> String {
    replacements
        .iter()
        .fold(source.to_string(), |text, replacement| {
            text.replace(&replacement.placeholder, &replacement.target)
        })
}

pub fn system_prompt_with_placeholders(
    custom_system_prompt: Option<&str>,
    target_language: Option<&str>,
    active_glossaries: bool,
) -> Option<String> {
    if !active_glossaries {
        return custom_system_prompt
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
            .map(ToOwned::to_owned);
    }

    let base = match custom_system_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        Some(prompt) => prompt.to_string(),
        None => {
            let language = target_language
                .and_then(koharu_llm::Language::parse)
                .unwrap_or(koharu_llm::Language::English);
            koharu_llm::prompt::base_system_prompt(language)
        }
    };
    Some(format!("{base}\n{PLACEHOLDER_SYSTEM_PROMPT}"))
}

fn ensure_dir(dir: &Utf8Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("failed to create `{dir}`"))
}

fn next_priority(config: &AppConfig) -> i32 {
    config
        .terminology_libraries
        .iter()
        .map(|library| library.priority)
        .max()
        .unwrap_or(0)
        + 10
}

fn read_csv_file(path: &Utf8Path) -> Result<Vec<TerminologyEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read `{path}`"))?;
    parse_csv(&bytes)
}

fn write_csv_file(path: &Utf8Path, terms: &[TerminologyEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, serialize_csv(terms)?).with_context(|| format!("failed to write `{path}`"))
}

pub fn parse_csv(bytes: &[u8]) -> Result<Vec<TerminologyEntry>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(bytes);
    let mut terms = Vec::new();
    for result in reader.records() {
        let record = result.context("failed to parse terminology CSV")?;
        let source = record.get(0).unwrap_or("").trim().to_string();
        let target = record.get(1).unwrap_or("").trim().to_string();
        if source.is_empty() {
            continue;
        }
        if terms.is_empty()
            && source.eq_ignore_ascii_case("source")
            && target.eq_ignore_ascii_case("target")
        {
            continue;
        }
        terms.push(TerminologyEntry { source, target });
    }
    Ok(terms)
}

pub fn serialize_csv(terms: &[TerminologyEntry]) -> Result<String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(["source", "target"])?;
    for term in terms {
        writer.write_record([term.source.as_str(), term.target.as_str()])?;
    }
    let bytes = writer.into_inner().context("failed to finish CSV writer")?;
    String::from_utf8(bytes).context("terminology CSV was not valid UTF-8")
}

fn sorted_terms(glossaries: &[ActiveGlossary]) -> Vec<&TerminologyEntry> {
    let mut candidates = glossaries
        .iter()
        .enumerate()
        .flat_map(|(library_index, library)| {
            library
                .terms
                .iter()
                .enumerate()
                .filter(|(_, term)| !term.source.is_empty())
                .map(move |(term_index, term)| {
                    (
                        library.priority,
                        term.source.len(),
                        library_index,
                        term_index,
                        term,
                    )
                })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(
        |(left_priority, left_len, left_library, left_term, _),
         (right_priority, right_len, right_library, right_term, _)| {
            right_priority
                .cmp(left_priority)
                .then_with(|| right_len.cmp(left_len))
                .then_with(|| left_library.cmp(right_library))
                .then_with(|| left_term.cmp(right_term))
        },
    );
    candidates
        .into_iter()
        .map(|(_, _, _, _, term)| term)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source: &str, target: &str) -> TerminologyEntry {
        TerminologyEntry {
            source: source.to_string(),
            target: target.to_string(),
        }
    }

    #[test]
    fn same_priority_uses_longest_match_first() {
        let protected = protect_text(
            "Apple Watch and Apple",
            &[ActiveGlossary {
                priority: 10,
                terms: vec![entry("Apple", "蘋果"), entry("Apple Watch", "蘋果手錶")],
            }],
        );

        assert_eq!(protected.text, "{{1}} and {{2}}");
        assert_eq!(
            protected.replacements,
            vec![
                PlaceholderReplacement {
                    placeholder: "{{1}}".to_string(),
                    target: "蘋果手錶".to_string(),
                },
                PlaceholderReplacement {
                    placeholder: "{{2}}".to_string(),
                    target: "蘋果".to_string(),
                },
            ]
        );
    }

    #[test]
    fn higher_priority_wins_before_longest_match() {
        let protected = protect_text(
            "Apple Watch",
            &[
                ActiveGlossary {
                    priority: 100,
                    terms: vec![entry("Apple", "高優先級蘋果")],
                },
                ActiveGlossary {
                    priority: 1,
                    terms: vec![entry("Apple Watch", "低優先級手錶")],
                },
            ],
        );

        assert_eq!(protected.text, "{{1}} Watch");
        assert_eq!(
            restore_text(&protected.text, &protected.replacements),
            "高優先級蘋果 Watch"
        );
    }

    #[test]
    fn csv_round_trip_uses_source_target_header() -> Result<()> {
        let csv = serialize_csv(&[entry("Apple", "蘋果")])?;
        assert_eq!(parse_csv(csv.as_bytes())?, vec![entry("Apple", "蘋果")]);
        Ok(())
    }
}
