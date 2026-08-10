use koharu_ml::llm::GenerationOptions;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Provider;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct GenerationConfig {
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub min_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub repeat_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub presence_penalty: Option<f32>,
    pub thinking: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct ModelSelection {
    pub provider: Provider,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub quantization: Option<String>,
    pub vision: bool,
}

impl Default for ModelSelection {
    fn default() -> Self {
        Self {
            provider: Provider::Local,
            model: Some(crate::local::DEFAULT_MODEL.to_owned()),
            quantization: Some(crate::local::DEFAULT_QUANTIZATION.to_owned()),
            vision: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct Quantization {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Type)]
pub struct Model {
    pub provider: Provider,
    pub model: Option<String>,
    pub name: String,
    pub quantizations: Vec<Quantization>,
    pub vision: bool,
}

impl Model {
    pub(crate) fn catalog(provider: Provider, entries: &[(&str, &str)], vision: bool) -> Vec<Self> {
        entries
            .iter()
            .map(|&(model, name)| Self {
                provider,
                model: Some(model.to_owned()),
                name: name.to_owned(),
                quantizations: Vec::new(),
                vision,
            })
            .collect()
    }

    pub(crate) fn service(provider: Provider, name: &str) -> Self {
        Self {
            provider,
            model: None,
            name: name.to_owned(),
            quantizations: Vec::new(),
            vision: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QuantizationDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub filename: &'static str,
}

impl QuantizationDefinition {
    pub const fn new(id: &'static str, name: &'static str, filename: &'static str) -> Self {
        Self { id, name, filename }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ModelGeneration {
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub min_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub repeat_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub presence_penalty: Option<f32>,
}

impl ModelGeneration {
    pub(crate) fn options(self, overrides: GenerationConfig) -> GenerationOptions {
        let defaults = GenerationOptions::default();
        GenerationOptions {
            max_tokens: overrides
                .max_tokens
                .or(self.max_tokens)
                .map_or(1000, |value| value as usize),
            temperature: overrides
                .temperature
                .or(self.temperature)
                .unwrap_or(defaults.temperature),
            top_k: overrides.top_k.or(self.top_k).map(|value| value as usize),
            top_p: overrides.top_p.or(self.top_p),
            min_p: overrides.min_p.or(self.min_p),
            repeat_penalty: overrides
                .repeat_penalty
                .or(self.repeat_penalty)
                .unwrap_or(defaults.repeat_penalty),
            frequency_penalty: overrides
                .frequency_penalty
                .or(self.frequency_penalty)
                .unwrap_or(defaults.frequency_penalty),
            presence_penalty: overrides
                .presence_penalty
                .or(self.presence_penalty)
                .unwrap_or(defaults.presence_penalty),
            ..defaults
        }
    }
}

#[must_use]
pub(crate) fn display_name(model: &str) -> String {
    model
        .rsplit_once('/')
        .map_or(model, |(_, model)| model)
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}
