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

define_local_models! {
    VntlLlama3_8Bv2 => {
        id: "vntl-llama3-8b-v2",
        repository: "lmg-anon/vntl-llama3-8b-v2-gguf",
        filename: "vntl-llama3-8b-v2-hf-q5_k_m.gguf",
        languages: SupportedLanguages::Limited(&[Language::English])
    };
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
    SakuraGalTransl7Bv3_7 => {
        id: "sakura-galtransl-7b-v3.7",
        repository: "SakuraLLM/Sakura-GalTransl-7B-v3.7",
        filename: "Sakura-Galtransl-7B-v3.7-IQ4_XS.gguf",
        languages: SupportedLanguages::Limited(&[Language::ChineseSimplified])
    };
    Sakura1_5bQwen2_5v1_0 => {
        id: "sakura-1.5b-qwen2.5-v1.0",
        repository: "shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX",
        filename: "sakura-1.5b-qwen2.5-v1.0-Q5KS.gguf",
        languages: SupportedLanguages::Limited(&[Language::ChineseSimplified])
    };
    HunyuanMT7B => {
        id: "hunyuan-mt-7b",
        repository: "Mungert/Hunyuan-MT-7B-GGUF",
        filename: "Hunyuan-MT-7B-q4_k_m.gguf",
        languages: SupportedLanguages::Limited(&[
            Language::ChineseSimplified,
            Language::English,
            Language::French,
            Language::Portuguese,
            Language::BrazilianPortuguese,
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
            Language::Malay,
            Language::Indonesian,
            Language::Filipino,
            Language::Hindi,
            Language::ChineseTraditional,
            Language::Polish,
            Language::Czech,
            Language::Dutch,
            Language::Khmer,
            Language::Burmese,
            Language::Persian,
            Language::Gujarati,
            Language::Urdu,
            Language::Telugu,
            Language::Marathi,
            Language::Hebrew,
            Language::Bengali,
            Language::Tamil,
            Language::Ukrainian,
            Language::Tibetan,
            Language::Kazakh,
            Language::Mongolian,
            Language::Uyghur,
            Language::Cantonese,
        ])
    };
    Sugoi14bUltra => {
        id: "sugoi-14b-ultra",
        repository: "sugoitoolkit/Sugoi-14B-Ultra-GGUF",
        filename: "Sugoi-14B-Ultra-Q4_K_M.gguf",
        languages: SupportedLanguages::Limited(&[Language::English])
    };
    Sugoi32bUltra => {
        id: "sugoi-32b-ultra",
        repository: "sugoitoolkit/Sugoi-32B-Ultra-GGUF",
        filename: "Sugoi-32B-Ultra-Q4_K_M.gguf",
        languages: SupportedLanguages::Limited(&[Language::English])
    };
    Gemma4E2bIt => {
        id: "gemma4-e2b-it",
        repository: "unsloth/gemma-4-E2B-it-GGUF",
        filename: "gemma-4-E2B-it-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4E4bIt => {
        id: "gemma4-e4b-it",
        repository: "unsloth/gemma-4-E4B-it-GGUF",
        filename: "gemma-4-E4B-it-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4_12bIt => {
        id: "gemma4-12b-it",
        repository: "unsloth/gemma-4-12b-it-GGUF",
        filename: "gemma-4-12b-it-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4_26bA4bIt => {
        id: "gemma4-26b-a4b-it",
        repository: "unsloth/gemma-4-26B-A4B-it-GGUF",
        filename: "gemma-4-26B-A4B-it-UD-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Gemma4_31bIt => {
        id: "gemma4-31b-it",
        repository: "unsloth/gemma-4-31B-it-GGUF",
        filename: "gemma-4-31B-it-Q4_K_M.gguf",
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
    Qwen3_5_0_8b => {
        id: "qwen3.5-0.8b",
        repository: "unsloth/Qwen3.5-0.8B-GGUF",
        filename: "Qwen3.5-0.8B-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_2b => {
        id: "qwen3.5-2b",
        repository: "unsloth/Qwen3.5-2B-GGUF",
        filename: "Qwen3.5-2B-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_4b => {
        id: "qwen3.5-4b",
        repository: "unsloth/Qwen3.5-4B-GGUF",
        filename: "Qwen3.5-4B-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_9b => {
        id: "qwen3.5-9b",
        repository: "unsloth/Qwen3.5-9B-GGUF",
        filename: "Qwen3.5-9B-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_27b => {
        id: "qwen3.5-27b",
        repository: "unsloth/Qwen3.5-27B-GGUF",
        filename: "Qwen3.5-27B-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_35bA3b => {
        id: "qwen3.5-35b-a3b",
        repository: "unsloth/Qwen3.5-35B-A3B-GGUF",
        filename: "Qwen3.5-35B-A3B-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_6_27b => {
        id: "qwen3.6-27b",
        repository: "unsloth/Qwen3.6-27B-GGUF",
        filename: "Qwen3.6-27B-IQ4_XS.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_6_35bA3b => {
        id: "qwen3.6-35b-a3b",
        repository: "unsloth/Qwen3.6-35B-A3B-GGUF",
        filename: "Qwen3.6-35B-A3B-UD-IQ4_XS.gguf",
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
    Qwen3_5_27bUncensored => {
        id: "qwen3.5-27b-uncensored",
        repository: "HauhauCS/Qwen3.5-27B-Uncensored-HauhauCS-Aggressive",
        filename: "Qwen3.5-27B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_5_35bA3bUncensored => {
        id: "qwen3.5-35b-a3b-uncensored",
        repository: "HauhauCS/Qwen3.5-35B-A3B-Uncensored-HauhauCS-Aggressive",
        filename: "Qwen3.5-35B-A3B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_6_27bUncensored => {
        id: "qwen3.6-27b-uncensored",
        repository: "HauhauCS/Qwen3.6-27B-Uncensored-HauhauCS-Aggressive",
        filename: "Qwen3.6-27B-Uncensored-HauhauCS-Aggressive-IQ4_XS.gguf",
        languages: SupportedLanguages::All
    };
    Qwen3_6_35bA3bUncensored => {
        id: "qwen3.6-35b-a3b-uncensored",
        repository: "HauhauCS/Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive",
        filename: "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-IQ4_XS.gguf",
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
            Gemma4E2bIt | Gemma4E4bIt | Gemma4_12bIt | Gemma4_26bA4bIt | Gemma4_31bIt
            | Gemma4E2bUncensored | Gemma4E4bUncensored => GenerationOptions {
                temperature: 1.0,
                top_k: Some(64),
                top_p: Some(0.95),
                repeat_penalty: 1.0,
                ..translation_generation_defaults()
            },
            Qwen3_5_0_8b
            | Qwen3_5_2b
            | Qwen3_5_4b
            | Qwen3_5_9b
            | Qwen3_5_27b
            | Qwen3_5_35bA3b
            | Qwen3_5_2bUncensored
            | Qwen3_5_4bUncensored
            | Qwen3_5_9bUncensored
            | Qwen3_5_27bUncensored
            | Qwen3_5_35bA3bUncensored => GenerationOptions {
                temperature: 1.0,
                top_k: Some(20),
                top_p: Some(1.0),
                min_p: Some(0.0),
                presence_penalty: 2.0,
                repeat_penalty: 1.0,
                ..translation_generation_defaults()
            },
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
            Sugoi14bUltra | Sugoi32bUltra => GenerationOptions {
                temperature: 0.1,
                top_k: Some(40),
                top_p: Some(0.95),
                min_p: Some(0.05),
                repeat_penalty: 1.1,
                ..translation_generation_defaults()
            },
            _ => translation_generation_defaults(),
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
            "gpt-5.5" => "GPT-5.5", "gpt-5.4" => "GPT-5.4",
            "gpt-5.4-mini" => "GPT-5.4 mini", "gpt-5.4-nano" => "GPT-5.4 nano",
            "gpt-5.2" => "GPT-5.2", "gpt-5.1" => "GPT-5.1", "gpt-5" => "GPT-5",
            "gpt-5-mini" => "GPT-5 mini", "gpt-5-nano" => "GPT-5 nano",
            "gpt-5-chat-latest" => "GPT-5 Chat latest", "gpt-4.1" => "GPT-4.1",
            "gpt-4.1-mini" => "GPT-4.1 mini", "gpt-4.1-nano" => "GPT-4.1 nano",
            "o3" => "o3", "o4-mini" => "o4-mini", "o3-mini" => "o3-mini",
            "o1" => "o1", "o1-mini" => "o1-mini", "o1-preview" => "o1 preview",
            "gpt-4o" => "GPT-4o", "gpt-4o-mini" => "GPT-4o mini",
            "gpt-4-turbo" => "GPT-4 Turbo", "gpt-4" => "GPT-4",
            "gpt-3.5-turbo" => "GPT-3.5 Turbo",
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
            "gemini-2.0-flash" => "Gemini 2.0 Flash",
            "gemini-2.0-flash-001" => "Gemini 2.0 Flash 001",
            "gemini-2.0-flash-lite" => "Gemini 2.0 Flash-Lite",
            "gemini-2.0-flash-lite-001" => "Gemini 2.0 Flash-Lite 001",
            "gemma-4-31b-it" => "Gemma 4 31B", "gemma-4-26b-a4b-it" => "Gemma 4 26B",
        ]),
        RemoteProviderKind::Claude => Some(remote_models![
            "claude-opus-4-7" => "Claude Opus 4.7",
            "claude-sonnet-4-6" => "Claude Sonnet 4.6",
            "claude-haiku-4-5" => "Claude Haiku 4.5",
            "claude-opus-4-6" => "Claude Opus 4.6",
            "claude-opus-4-5-20251101" => "Claude Opus 4.5",
            "claude-opus-4-1-20250805" => "Claude Opus 4.1",
            "claude-sonnet-4-5-20250929" => "Claude Sonnet 4.5",
            "claude-haiku-4-5-20251001" => "Claude Haiku 4.5 snapshot",
            "claude-opus-4-20250514" => "Claude Opus 4 (deprecated)",
            "claude-sonnet-4-20250514" => "Claude Sonnet 4 (deprecated)",
        ]),
        RemoteProviderKind::DeepSeek => Some(remote_models![
            "deepseek-v4-flash" => "DeepSeek V4 Flash",
            "deepseek-v4-pro" => "DeepSeek V4 Pro",
            "deepseek-chat" => "DeepSeek Chat",
            "deepseek-reasoner" => "DeepSeek Reasoner",
        ]),
        RemoteProviderKind::OpenAiCompatible => None,
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
        assert_eq!(local_models().len(), 29);
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
    fn catalogs_cover_main_branch_provider_families() {
        assert_eq!(remote_providers().len(), 8);
        assert!(
            RemoteProviderKind::OpenAiCompatible
                .descriptor()
                .discovers_models
        );
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
                .any(|model| model.id == "gpt-5.5")
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
                .any(|model| model.id == "claude-opus-4-7")
        );
        assert!(
            remote_models(RemoteProviderKind::DeepSeek)
                .unwrap()
                .iter()
                .any(|model| model.id == "deepseek-v4-pro")
        );
        assert!(remote_models(RemoteProviderKind::OpenAiCompatible).is_none());
    }

    #[test]
    fn gemma_12b_receives_gemma_sampling_defaults() {
        let options = LocalModel::Gemma4_12bIt.generation_options();
        assert_eq!(options.temperature, 1.0);
        assert_eq!(options.top_k, Some(64));
        assert_eq!(options.top_p, Some(0.95));
    }
}
