use std::path::PathBuf;

use koharu_ml::llm::GenerationOptions;
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};

use crate::Language;

/// Language coverage advertised by a model or provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedLanguages {
    All,
    Limited(&'static [Language]),
}

impl SupportedLanguages {
    #[must_use]
    pub fn contains(&self, language: Language) -> bool {
        match self {
            Self::All => true,
            Self::Limited(languages) => languages.contains(&language),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalModelDescriptor {
    pub model: LocalModel,
    pub id: &'static str,
    pub repository: &'static str,
    pub filename: &'static str,
    pub target_languages: SupportedLanguages,
}

// Catalog ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/lib.rs
macro_rules! define_local_models {
    ($(
        $variant:ident => {
            id: $id:literal,
            repository: $repository:literal,
            filename: $filename:literal,
            languages: $languages:expr
        };
    )+) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, EnumIter,
        )]
        pub enum LocalModel {
            $(#[strum(serialize = $id)] $variant,)+
        }

        impl LocalModel {
            #[must_use]
            pub fn descriptor(self) -> LocalModelDescriptor {
                match self {
                    $(Self::$variant => LocalModelDescriptor {
                        model: self,
                        id: $id,
                        repository: $repository,
                        filename: $filename,
                        target_languages: $languages,
                    },)+
                }
            }
        }
    };
}

// Use each publisher's documented default or explicitly recommended GGUF.
// Prefer QAT-derived artifacts when the publisher provides them.
define_local_models! {
    Lfm2_5_1_2bInstruct => {
        id: "lfm2.5-1.2b-instruct",
        repository: "LiquidAI/LFM2.5-1.2B-Instruct-GGUF",
        filename: "LFM2.5-1.2B-Instruct-Q4_K_M.gguf",
        languages: SupportedLanguages::Limited(&[
            Language::English,
            Language::Arabic,
            Language::ChineseSimplified,
            Language::French,
            Language::German,
            Language::Japanese,
            Language::Korean,
            Language::Portuguese,
            Language::Spanish,
        ])
    };
    Ministral3_8bInstruct => {
        id: "ministral-3-8b-instruct",
        repository: "mistralai/Ministral-3-8B-Instruct-2512-GGUF",
        filename: "Ministral-3-8B-Instruct-2512-Q4_K_M.gguf",
        languages: SupportedLanguages::Limited(&[
            Language::English,
            Language::Arabic,
            Language::ChineseSimplified,
            Language::French,
            Language::German,
            Language::Italian,
            Language::Japanese,
            Language::Korean,
            Language::Portuguese,
            Language::Spanish,
            Language::Dutch,
        ])
    };
    Gemma4E2bIt => {
        id: "gemma4-e2b-it",
        repository: "unsloth/gemma-4-E2B-it-qat-GGUF",
        filename: "gemma-4-E2B-it-qat-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4E4bIt => {
        id: "gemma4-e4b-it",
        repository: "unsloth/gemma-4-E4B-it-qat-GGUF",
        filename: "gemma-4-E4B-it-qat-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4_12bIt => {
        id: "gemma4-12b-it",
        repository: "unsloth/gemma-4-12B-it-qat-GGUF",
        filename: "gemma-4-12B-it-qat-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4_26bA4bIt => {
        id: "gemma4-26b-a4b-it",
        repository: "unsloth/gemma-4-26B-A4B-it-qat-GGUF",
        filename: "gemma-4-26B-A4B-it-qat-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4_31bIt => {
        id: "gemma4-31b-it",
        repository: "unsloth/gemma-4-31B-it-qat-GGUF",
        filename: "gemma-4-31B-it-qat-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4E2bUncensored => {
        id: "gemma4-e2b-uncensored",
        repository: "HauhauCS/Gemma-4-E2B-Uncensored-HauhauCS-Aggressive",
        filename: "Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-Q4_K_P.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4E4bUncensored => {
        id: "gemma4-e4b-uncensored",
        repository: "HauhauCS/Gemma-4-E4B-Uncensored-HauhauCS-Aggressive",
        filename: "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4_12bUncensored => {
        id: "gemma4-12b-uncensored",
        repository: "HauhauCS/Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced",
        filename: "Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4_26bA4bUncensored => {
        id: "gemma4-26b-a4b-uncensored",
        repository: "HauhauCS/Gemma4-26B-A4B-QAT-Uncensored-HauhauCS-Balanced-MTP",
        filename: "Gemma4-26B-A4B-QAT-Uncensored-HauhauCS-Balanced-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4_31bUncensored => {
        id: "gemma4-31b-uncensored",
        repository: "HauhauCS/Gemma4-31B-QAT-Uncensored-HauhauCS-Balanced-MTP",
        filename: "Gemma4-31B-QAT-Uncensored-HauhauCS-Balanced-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_0_8b => {
        id: "qwen3.5-0.8b",
        repository: "unsloth/Qwen3.5-0.8B-GGUF",
        filename: "Qwen3.5-0.8B-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_2b => {
        id: "qwen3.5-2b",
        repository: "unsloth/Qwen3.5-2B-GGUF",
        filename: "Qwen3.5-2B-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_4b => {
        id: "qwen3.5-4b",
        repository: "unsloth/Qwen3.5-4B-GGUF",
        filename: "Qwen3.5-4B-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_9b => {
        id: "qwen3.5-9b",
        repository: "unsloth/Qwen3.5-9B-GGUF",
        filename: "Qwen3.5-9B-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_27b => {
        id: "qwen3.5-27b",
        repository: "unsloth/Qwen3.5-27B-GGUF",
        filename: "Qwen3.5-27B-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_35bA3b => {
        id: "qwen3.5-35b-a3b",
        repository: "unsloth/Qwen3.5-35B-A3B-GGUF",
        filename: "Qwen3.5-35B-A3B-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_6_27b => {
        id: "qwen3.6-27b",
        repository: "unsloth/Qwen3.6-27B-GGUF",
        filename: "Qwen3.6-27B-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_6_35bA3b => {
        id: "qwen3.6-35b-a3b",
        repository: "unsloth/Qwen3.6-35B-A3B-GGUF",
        filename: "Qwen3.6-35B-A3B-UD-Q4_K_XL.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_2bUncensored => {
        id: "qwen3.5-2b-uncensored",
        repository: "HauhauCS/Qwen3.5-2B-Uncensored-HauhauCS-Aggressive",
        filename: "Qwen3.5-2B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_4bUncensored => {
        id: "qwen3.5-4b-uncensored",
        repository: "HauhauCS/Qwen3.5-4B-Uncensored-HauhauCS-Aggressive",
        filename: "Qwen3.5-4B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_9bUncensored => {
        id: "qwen3.5-9b-uncensored",
        repository: "HauhauCS/Qwen3.5-9B-Uncensored-HauhauCS-Aggressive",
        filename: "Qwen3.5-9B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_6_27bUncensored => {
        id: "qwen3.6-27b-uncensored",
        repository: "HauhauCS/Qwen3.6-27B-Uncensored-HauhauCS-Balanced",
        filename: "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-Q4_K_P.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_6_35bA3bUncensored => {
        id: "qwen3.6-35b-a3b-uncensored",
        repository: "HauhauCS/Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive",
        filename: "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-Q4_K_P.gguf",
        languages: SupportedLanguages::All
    };
}

impl LocalModel {
    #[must_use]
    pub fn id(self) -> &'static str {
        self.descriptor().id
    }

    pub async fn resolve(self) -> anyhow::Result<PathBuf> {
        let descriptor = self.descriptor();
        koharu_runtime::package::huggingface::resolve((descriptor.repository, descriptor.filename))
            .await
    }

    #[must_use]
    pub fn generation_options(self) -> GenerationOptions {
        use LocalModel::*;

        match self {
            Lfm2_5_1_2bInstruct => GenerationOptions {
                temperature: 0.1,
                top_k: Some(50),
                repeat_penalty: 1.05,
                ..translation_generation_defaults()
            },
            Ministral3_8bInstruct => GenerationOptions {
                temperature: 0.05,
                ..translation_generation_defaults()
            },
            Gemma4E2bIt | Gemma4E4bIt | Gemma4_12bIt | Gemma4_26bA4bIt | Gemma4_31bIt
            | Gemma4E2bUncensored | Gemma4E4bUncensored => GenerationOptions {
                temperature: 1.0,
                top_k: Some(64),
                top_p: Some(0.95),
                repeat_penalty: 1.0,
                ..translation_generation_defaults()
            },
            Gemma4_12bUncensored | Gemma4_26bA4bUncensored | Gemma4_31bUncensored => {
                GenerationOptions {
                    temperature: 0.6,
                    top_k: Some(64),
                    top_p: Some(0.9),
                    min_p: Some(0.05),
                    repeat_penalty: 1.1,
                    ..translation_generation_defaults()
                }
            }
            Qwen3_5_0_8b | Qwen3_5_2b | Qwen3_5_4b | Qwen3_5_9b | Qwen3_5_27b | Qwen3_5_35bA3b
            | Qwen3_5_2bUncensored | Qwen3_5_4bUncensored | Qwen3_5_9bUncensored => {
                GenerationOptions {
                    temperature: 1.0,
                    top_k: Some(20),
                    top_p: Some(1.0),
                    min_p: Some(0.0),
                    presence_penalty: 2.0,
                    repeat_penalty: 1.0,
                    ..translation_generation_defaults()
                }
            }
            Qwen3_6_27b | Qwen3_6_35bA3b | Qwen3_6_27bUncensored | Qwen3_6_35bA3bUncensored => {
                GenerationOptions {
                    temperature: 0.7,
                    top_k: Some(20),
                    top_p: Some(0.8),
                    min_p: Some(0.0),
                    presence_penalty: 1.5,
                    repeat_penalty: 1.0,
                    ..translation_generation_defaults()
                }
            }
        }
    }
}

fn translation_generation_defaults() -> GenerationOptions {
    GenerationOptions {
        max_tokens: 1000,
        ..Default::default()
    }
}

#[must_use]
pub fn local_models() -> Vec<LocalModelDescriptor> {
    LocalModel::iter().map(LocalModel::descriptor).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, EnumIter)]
pub enum RemoteProviderKind {
    #[strum(serialize = "openai")]
    OpenAi,
    #[strum(serialize = "gemini")]
    Gemini,
    #[strum(serialize = "claude")]
    Claude,
    #[strum(serialize = "deepseek")]
    DeepSeek,
    #[strum(serialize = "openai-compatible")]
    OpenAiCompatible,
    #[strum(serialize = "openrouter")]
    OpenRouter,
    #[strum(serialize = "lm-studio")]
    LmStudio,
    #[strum(serialize = "deepl")]
    DeepL,
    #[strum(serialize = "google-cloud-translation")]
    GoogleCloudTranslation,
    #[strum(serialize = "caiyun")]
    Caiyun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteProviderDescriptor {
    pub kind: RemoteProviderKind,
    pub id: &'static str,
    pub name: &'static str,
    pub requires_api_key: bool,
    pub requires_base_url: bool,
    pub target_languages: SupportedLanguages,
    pub supports_context: bool,
    /// `true` when models must be read from the configured endpoint.
    pub discovers_models: bool,
}

impl RemoteProviderKind {
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::Claude => "claude",
            Self::DeepSeek => "deepseek",
            Self::OpenAiCompatible => "openai-compatible",
            Self::OpenRouter => "openrouter",
            Self::LmStudio => "lm-studio",
            Self::DeepL => "deepl",
            Self::GoogleCloudTranslation => "google-cloud-translation",
            Self::Caiyun => "caiyun",
        }
    }

    #[must_use]
    pub fn descriptor(self) -> RemoteProviderDescriptor {
        let (
            name,
            requires_api_key,
            requires_base_url,
            target_languages,
            supports_context,
            discovers_models,
        ) = match self {
            Self::OpenAi => ("OpenAI", true, false, SupportedLanguages::All, true, false),
            Self::Gemini => ("Gemini", true, false, SupportedLanguages::All, true, false),
            Self::Claude => ("Claude", true, false, SupportedLanguages::All, true, false),
            Self::DeepSeek => (
                "DeepSeek",
                true,
                false,
                SupportedLanguages::All,
                true,
                false,
            ),
            Self::OpenAiCompatible => (
                "OpenAI-compatible",
                false,
                true,
                SupportedLanguages::All,
                true,
                true,
            ),
            Self::OpenRouter => (
                "OpenRouter",
                true,
                false,
                SupportedLanguages::All,
                true,
                true,
            ),
            Self::LmStudio => (
                "LM Studio",
                false,
                true,
                SupportedLanguages::All,
                true,
                true,
            ),
            Self::DeepL => ("DeepL", true, false, SupportedLanguages::All, true, false),
            Self::GoogleCloudTranslation => (
                "Google Cloud Translation",
                true,
                false,
                SupportedLanguages::All,
                false,
                false,
            ),
            Self::Caiyun => (
                "Caiyun",
                true,
                false,
                SupportedLanguages::Limited(&[
                    Language::ChineseSimplified,
                    Language::English,
                    Language::French,
                    Language::Portuguese,
                    Language::Spanish,
                    Language::Japanese,
                    Language::Turkish,
                    Language::Russian,
                    Language::Arabic,
                    Language::Korean,
                    Language::Thai,
                    Language::Italian,
                    Language::German,
                    Language::Vietnamese,
                    Language::Indonesian,
                    Language::ChineseTraditional,
                    Language::Polish,
                ]),
                false,
                false,
            ),
        };
        RemoteProviderDescriptor {
            kind: self,
            id: self.id(),
            name,
            requires_api_key,
            requires_base_url,
            target_languages,
            supports_context,
            discovers_models,
        }
    }
}

#[must_use]
pub fn remote_providers() -> Vec<RemoteProviderDescriptor> {
    RemoteProviderKind::iter()
        .map(RemoteProviderKind::descriptor)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteModelDescriptor {
    pub id: &'static str,
    pub name: &'static str,
}

macro_rules! remote_models {
    ($( $id:literal => $name:literal ),+ $(,)?) => {
        &[$(RemoteModelDescriptor { id: $id, name: $name }),+]
    };
}

/// Returns static model choices, or `None` when models must be discovered from the endpoint.
#[must_use]
pub fn remote_models(provider: RemoteProviderKind) -> Option<&'static [RemoteModelDescriptor]> {
    match provider {
        RemoteProviderKind::OpenAi => Some(remote_models![
            "gpt-5.6" => "GPT-5.6 (Sol)", "gpt-5.6-sol" => "GPT-5.6 Sol",
            "gpt-5.6-terra" => "GPT-5.6 Terra", "gpt-5.6-luna" => "GPT-5.6 Luna",
            "gpt-5.5" => "GPT-5.5", "gpt-5.4" => "GPT-5.4",
            "gpt-5.4-mini" => "GPT-5.4 mini", "gpt-5.4-nano" => "GPT-5.4 nano",
            "gpt-5.2" => "GPT-5.2", "gpt-5.1" => "GPT-5.1", "gpt-5" => "GPT-5",
            "gpt-5-mini" => "GPT-5 mini", "gpt-5-nano" => "GPT-5 nano",
            "o3" => "o3", "gpt-4.1" => "GPT-4.1",
            "gpt-4.1-mini" => "GPT-4.1 mini", "gpt-4o-mini" => "GPT-4o mini",
        ]),
        RemoteProviderKind::Gemini => Some(remote_models![
            "gemini-flash-lite-latest" => "Gemini Flash-Lite Latest",
            "gemini-flash-latest" => "Gemini Flash Latest",
            "gemini-pro-latest" => "Gemini Pro Latest",
            "gemini-3.5-flash" => "Gemini 3.5 Flash",
            "gemini-3.1-pro-preview" => "Gemini 3.1 Pro Preview",
            "gemini-3.1-pro-preview-customtools" => "Gemini 3.1 Pro Preview Custom Tools",
            "gemini-3.1-flash-lite" => "Gemini 3.1 Flash-Lite",
            "gemini-3-flash-preview" => "Gemini 3 Flash Preview",
            "gemini-2.5-pro" => "Gemini 2.5 Pro", "gemini-2.5-flash" => "Gemini 2.5 Flash",
            "gemini-2.5-flash-lite" => "Gemini 2.5 Flash-Lite",
        ]),
        RemoteProviderKind::Claude => Some(remote_models![
            "claude-fable-5" => "Claude Fable 5",
            "claude-opus-4-8" => "Claude Opus 4.8",
            "claude-sonnet-5" => "Claude Sonnet 5",
            "claude-haiku-4-5" => "Claude Haiku 4.5",
            "claude-opus-4-7" => "Claude Opus 4.7",
            "claude-sonnet-4-6" => "Claude Sonnet 4.6",
            "claude-opus-4-6" => "Claude Opus 4.6",
            "claude-opus-4-5-20251101" => "Claude Opus 4.5",
            "claude-haiku-4-5-20251001" => "Claude Haiku 4.5 snapshot",
        ]),
        RemoteProviderKind::DeepSeek => Some(remote_models![
            "deepseek-v4-flash" => "DeepSeek V4 Flash",
            "deepseek-v4-pro" => "DeepSeek V4 Pro",
        ]),
        RemoteProviderKind::OpenAiCompatible
        | RemoteProviderKind::OpenRouter
        | RemoteProviderKind::LmStudio => None,
        RemoteProviderKind::DeepL
        | RemoteProviderKind::GoogleCloudTranslation
        | RemoteProviderKind::Caiyun => Some(remote_models!["mt" => "Machine Translation"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_catalog_has_unique_complete_entries() {
        assert_eq!(local_models().len(), 25);
        for (index, model) in local_models().iter().enumerate() {
            assert!(!model.id.is_empty());
            assert!(!model.repository.is_empty());
            assert!(!model.filename.is_empty());
            assert!(
                !local_models()[..index]
                    .iter()
                    .any(|other| other.id == model.id)
            );
            assert_eq!(model.id.parse::<LocalModel>().unwrap(), model.model);
        }
    }

    #[test]
    fn ministral_8b_uses_official_gguf_and_production_sampling() {
        let descriptor = LocalModel::Ministral3_8bInstruct.descriptor();
        assert_eq!(
            descriptor.repository,
            "mistralai/Ministral-3-8B-Instruct-2512-GGUF"
        );
        assert_eq!(
            descriptor.filename,
            "Ministral-3-8B-Instruct-2512-Q4_K_M.gguf"
        );
        assert!(descriptor.target_languages.contains(Language::Japanese));
        assert!(!descriptor.target_languages.contains(Language::Vietnamese));

        let options = descriptor.model.generation_options();
        assert!(options.temperature < 0.1);
    }

    #[test]
    fn catalogs_cover_main_branch_provider_families() {
        assert_eq!(remote_providers().len(), 10);
        assert!(
            RemoteProviderKind::OpenAiCompatible
                .descriptor()
                .discovers_models
        );
        assert!(RemoteProviderKind::OpenRouter.descriptor().discovers_models);
        assert!(RemoteProviderKind::LmStudio.descriptor().discovers_models);
        assert!(
            RemoteProviderKind::Caiyun
                .descriptor()
                .target_languages
                .contains(Language::Japanese)
        );
        assert!(
            !RemoteProviderKind::Caiyun
                .descriptor()
                .target_languages
                .contains(Language::Hungarian)
        );
        assert!(RemoteProviderKind::OpenAi.descriptor().supports_context);
        assert!(RemoteProviderKind::DeepL.descriptor().supports_context);
        assert!(!RemoteProviderKind::Caiyun.descriptor().supports_context);
        assert!(
            remote_models(RemoteProviderKind::OpenAi)
                .unwrap()
                .iter()
                .any(|model| model.id == "gpt-5.6-sol")
        );
        assert!(
            remote_models(RemoteProviderKind::Gemini)
                .unwrap()
                .iter()
                .any(|model| model.id == "gemini-3.5-flash")
        );
        assert!(
            remote_models(RemoteProviderKind::Claude)
                .unwrap()
                .iter()
                .any(|model| model.id == "claude-opus-4-8")
        );
        assert!(
            remote_models(RemoteProviderKind::DeepSeek)
                .unwrap()
                .iter()
                .any(|model| model.id == "deepseek-v4-pro")
        );
        assert!(
            remote_models(RemoteProviderKind::DeepSeek)
                .unwrap()
                .iter()
                .all(|model| !matches!(model.id, "deepseek-chat" | "deepseek-reasoner"))
        );
        assert!(
            remote_models(RemoteProviderKind::Gemini)
                .unwrap()
                .iter()
                .all(|model| !model.id.starts_with("gemini-2.0"))
        );
        assert!(remote_models(RemoteProviderKind::OpenAiCompatible).is_none());
        assert!(remote_models(RemoteProviderKind::OpenRouter).is_none());
        assert!(remote_models(RemoteProviderKind::LmStudio).is_none());
    }

    #[test]
    fn gemma_12b_receives_gemma_sampling_defaults() {
        let options = LocalModel::Gemma4_12bIt.generation_options();
        assert_eq!(options.temperature, 1.0);
        assert_eq!(options.top_k, Some(64));
        assert_eq!(options.top_p, Some(0.95));
    }

    #[test]
    fn gemma_instruct_models_use_recommended_dynamic_qat_artifacts() {
        let expected = [
            (
                LocalModel::Gemma4E2bIt,
                "unsloth/gemma-4-E2B-it-qat-GGUF",
                "gemma-4-E2B-it-qat-UD-Q4_K_XL.gguf",
            ),
            (
                LocalModel::Gemma4E4bIt,
                "unsloth/gemma-4-E4B-it-qat-GGUF",
                "gemma-4-E4B-it-qat-UD-Q4_K_XL.gguf",
            ),
            (
                LocalModel::Gemma4_12bIt,
                "unsloth/gemma-4-12B-it-qat-GGUF",
                "gemma-4-12B-it-qat-UD-Q4_K_XL.gguf",
            ),
            (
                LocalModel::Gemma4_26bA4bIt,
                "unsloth/gemma-4-26B-A4B-it-qat-GGUF",
                "gemma-4-26B-A4B-it-qat-UD-Q4_K_XL.gguf",
            ),
            (
                LocalModel::Gemma4_31bIt,
                "unsloth/gemma-4-31B-it-qat-GGUF",
                "gemma-4-31B-it-qat-UD-Q4_K_XL.gguf",
            ),
        ];

        for (model, repository, filename) in expected {
            let descriptor = model.descriptor();
            assert_eq!(descriptor.repository, repository);
            assert_eq!(descriptor.filename, filename);
        }
    }

    #[test]
    fn hauhau_gemma_models_use_recommended_qat_artifacts() {
        let expected = [
            (
                LocalModel::Gemma4_12bUncensored,
                "HauhauCS/Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced",
                "Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced-Q4_K_M.gguf",
            ),
            (
                LocalModel::Gemma4_26bA4bUncensored,
                "HauhauCS/Gemma4-26B-A4B-QAT-Uncensored-HauhauCS-Balanced-MTP",
                "Gemma4-26B-A4B-QAT-Uncensored-HauhauCS-Balanced-Q4_K_M.gguf",
            ),
            (
                LocalModel::Gemma4_31bUncensored,
                "HauhauCS/Gemma4-31B-QAT-Uncensored-HauhauCS-Balanced-MTP",
                "Gemma4-31B-QAT-Uncensored-HauhauCS-Balanced-Q4_K_M.gguf",
            ),
        ];

        for (model, repository, filename) in expected {
            let descriptor = model.descriptor();
            assert_eq!(descriptor.repository, repository);
            assert_eq!(descriptor.filename, filename);

            let options = model.generation_options();
            assert_eq!(options.temperature, 0.6);
            assert_eq!(options.top_k, Some(64));
            assert_eq!(options.top_p, Some(0.9));
            assert_eq!(options.min_p, Some(0.05));
            assert_eq!(options.repeat_penalty, 1.1);
        }
    }

    #[test]
    fn unsloth_qwen_models_use_recommended_dynamic_quant() {
        let models = [
            LocalModel::Qwen3_5_0_8b,
            LocalModel::Qwen3_5_2b,
            LocalModel::Qwen3_5_4b,
            LocalModel::Qwen3_5_9b,
            LocalModel::Qwen3_5_27b,
            LocalModel::Qwen3_5_35bA3b,
            LocalModel::Qwen3_6_27b,
            LocalModel::Qwen3_6_35bA3b,
        ];

        for model in models {
            let descriptor = model.descriptor();
            assert!(descriptor.repository.starts_with("unsloth/Qwen3."));
            assert!(descriptor.filename.ends_with("-UD-Q4_K_XL.gguf"));
        }
    }

    #[test]
    fn qwen_uncensored_models_use_publisher_recommendations() {
        let qwen_27b = LocalModel::Qwen3_6_27bUncensored.descriptor();
        assert_eq!(
            qwen_27b.repository,
            "HauhauCS/Qwen3.6-27B-Uncensored-HauhauCS-Balanced"
        );
        assert_eq!(
            qwen_27b.filename,
            "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-Q4_K_P.gguf"
        );

        assert_eq!(
            LocalModel::Qwen3_6_35bA3bUncensored.descriptor().filename,
            "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-Q4_K_P.gguf"
        );
    }
}
