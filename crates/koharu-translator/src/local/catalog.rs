use std::path::PathBuf;

use crate::{ModelGeneration, ModelSelection, QuantizationDefinition};

pub(crate) const DEFAULT_MODEL: &str = "gemma4-12b-it";
pub(crate) const DEFAULT_QUANTIZATION: &str = "Q4_K_XL";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupportedLanguages {
    All,
    Limited(&'static [crate::Language]),
}

impl SupportedLanguages {
    #[must_use]
    pub(crate) fn contains(self, language: crate::Language) -> bool {
        match self {
            Self::All => true,
            Self::Limited(languages) => languages.contains(&language),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LocalModelDescriptor {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) quantizations: &'static [QuantizationDefinition],
    pub(crate) generation: ModelGeneration,
    pub(crate) repository: &'static str,
    pub(crate) projector: Option<&'static str>,
    pub(crate) target_languages: SupportedLanguages,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct LocalConfig {}

pub(super) static MODELS: &[LocalModelDescriptor] = &[
    LocalModelDescriptor {
        id: "lfm2.5-1.2b-instruct",
        name: "LFM2.5 1.2B Instruct",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_M", "Q4_K_M", "LFM2.5-1.2B-Instruct-Q4_K_M.gguf"),
            QuantizationDefinition::new("BF16", "BF16", "LFM2.5-1.2B-Instruct-BF16.gguf"),
            QuantizationDefinition::new("F16", "F16", "LFM2.5-1.2B-Instruct-F16.gguf"),
            QuantizationDefinition::new("Q4_0", "Q4_0", "LFM2.5-1.2B-Instruct-Q4_0.gguf"),
            QuantizationDefinition::new("Q5_K_M", "Q5_K_M", "LFM2.5-1.2B-Instruct-Q5_K_M.gguf"),
            QuantizationDefinition::new("Q6_K", "Q6_K", "LFM2.5-1.2B-Instruct-Q6_K.gguf"),
            QuantizationDefinition::new("Q8_0", "Q8_0", "LFM2.5-1.2B-Instruct-Q8_0.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(0.1),
            top_k: Some(50),
            top_p: None,
            min_p: None,
            max_tokens: Some(1000),
            repeat_penalty: Some(1.05),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "LiquidAI/LFM2.5-1.2B-Instruct-GGUF",
        projector: None,
        target_languages: SupportedLanguages::Limited(&[
            crate::Language::English,
            crate::Language::Arabic,
            crate::Language::ChineseSimplified,
            crate::Language::French,
            crate::Language::German,
            crate::Language::Japanese,
            crate::Language::Korean,
            crate::Language::Portuguese,
            crate::Language::Spanish,
        ]),
    },
    LocalModelDescriptor {
        id: "ministral-3-8b-instruct",
        name: "Ministral 3 8B Instruct",
        quantizations: &[
            QuantizationDefinition::new(
                "Q4_K_M",
                "Q4_K M",
                "Ministral-3-8B-Instruct-2512-Q4_K_M.gguf",
            ),
            QuantizationDefinition::new("BF16", "BF16", "Ministral-3-8B-Instruct-2512-BF16.gguf"),
            QuantizationDefinition::new(
                "Q5_K_M",
                "Q5_K M",
                "Ministral-3-8B-Instruct-2512-Q5_K_M.gguf",
            ),
            QuantizationDefinition::new("Q8_0", "Q8_0", "Ministral-3-8B-Instruct-2512-Q8_0.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(0.05),
            top_k: None,
            top_p: None,
            min_p: None,
            max_tokens: Some(1000),
            repeat_penalty: None,
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "mistralai/Ministral-3-8B-Instruct-2512-GGUF",
        projector: None,
        target_languages: SupportedLanguages::Limited(&[
            crate::Language::English,
            crate::Language::Arabic,
            crate::Language::ChineseSimplified,
            crate::Language::French,
            crate::Language::German,
            crate::Language::Italian,
            crate::Language::Japanese,
            crate::Language::Korean,
            crate::Language::Portuguese,
            crate::Language::Spanish,
            crate::Language::Dutch,
        ]),
    },
    LocalModelDescriptor {
        id: "gemma4-e2b-it",
        name: "Gemma 4 E2B Instruct",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "gemma-4-E2B-it-qat-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "gemma-4-E2B-it-qat-UD-Q2_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            min_p: None,
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "unsloth/gemma-4-E2B-it-qat-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "gemma4-e4b-it",
        name: "Gemma 4 E4B Instruct",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "gemma-4-E4B-it-qat-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "gemma-4-E4B-it-qat-UD-Q2_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            min_p: None,
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "unsloth/gemma-4-E4B-it-qat-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "gemma4-12b-it",
        name: "Gemma 4 12B Instruct",
        quantizations: &[QuantizationDefinition::new(
            "Q4_K_XL",
            "Q4_K XL",
            "gemma-4-12B-it-qat-UD-Q4_K_XL.gguf",
        )],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            min_p: None,
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "unsloth/gemma-4-12B-it-qat-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "gemma4-26b-a4b-it",
        name: "Gemma 4 26B A4B Instruct",
        quantizations: &[QuantizationDefinition::new(
            "Q4_K_XL",
            "Q4_K XL",
            "gemma-4-26B-A4B-it-qat-UD-Q4_K_XL.gguf",
        )],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            min_p: None,
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "unsloth/gemma-4-26B-A4B-it-qat-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "gemma4-31b-it",
        name: "Gemma 4 31B Instruct",
        quantizations: &[QuantizationDefinition::new(
            "Q4_K_XL",
            "Q4_K XL",
            "gemma-4-31B-it-qat-UD-Q4_K_XL.gguf",
        )],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            min_p: None,
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "unsloth/gemma-4-31B-it-qat-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "gemma4-e2b-uncensored",
        name: "Gemma 4 E2B Uncensored",
        quantizations: &[
            QuantizationDefinition::new(
                "Q4_K_P",
                "Q4_K P",
                "Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-Q4_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "IQ3_M",
                "IQ3 M",
                "Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-IQ3_M.gguf",
            ),
            QuantizationDefinition::new(
                "Q2_K_P",
                "Q2_K P",
                "Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-Q2_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q3_K_P",
                "Q3_K P",
                "Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-Q3_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q5_K_P",
                "Q5_K P",
                "Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-Q5_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q6_K_P",
                "Q6_K P",
                "Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-Q6_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q8_K_P",
                "Q8_K P",
                "Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-Q8_K_P.gguf",
            ),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            min_p: None,
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "HauhauCS/Gemma-4-E2B-Uncensored-HauhauCS-Aggressive",
        projector: Some("mmproj-Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-f16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "gemma4-e4b-uncensored",
        name: "Gemma 4 E4B Uncensored",
        quantizations: &[
            QuantizationDefinition::new(
                "Q4_K_P",
                "Q4_K P",
                "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-Q4_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "IQ3_M",
                "IQ3 M",
                "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-IQ3_M.gguf",
            ),
            QuantizationDefinition::new(
                "IQ4_XS",
                "IQ4 XS",
                "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-IQ4_XS.gguf",
            ),
            QuantizationDefinition::new(
                "Q2_K_P",
                "Q2_K P",
                "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-Q2_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q3_K_P",
                "Q3_K P",
                "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-Q3_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q5_K_P",
                "Q5_K P",
                "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-Q5_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q6_K_P",
                "Q6_K P",
                "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-Q6_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q8_K_P",
                "Q8_K P",
                "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-Q8_K_P.gguf",
            ),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(64),
            top_p: Some(0.95),
            min_p: None,
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "HauhauCS/Gemma-4-E4B-Uncensored-HauhauCS-Aggressive",
        projector: Some("mmproj-Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-f16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "gemma4-12b-uncensored",
        name: "Gemma 4 12B Uncensored",
        quantizations: &[QuantizationDefinition::new(
            "Q4_K_M",
            "Q4_K M",
            "Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced-Q4_K_M.gguf",
        )],
        generation: ModelGeneration {
            temperature: Some(0.6),
            top_k: Some(64),
            top_p: Some(0.9),
            min_p: Some(0.05),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.1),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "HauhauCS/Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced",
        projector: Some("mmproj-Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced-BF16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "gemma4-26b-a4b-uncensored",
        name: "Gemma 4 26B A4B Uncensored",
        quantizations: &[QuantizationDefinition::new(
            "Q4_K_M",
            "Q4_K M",
            "Gemma4-26B-A4B-QAT-Uncensored-HauhauCS-Balanced-Q4_K_M.gguf",
        )],
        generation: ModelGeneration {
            temperature: Some(0.6),
            top_k: Some(64),
            top_p: Some(0.9),
            min_p: Some(0.05),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.1),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "HauhauCS/Gemma4-26B-A4B-QAT-Uncensored-HauhauCS-Balanced-MTP",
        projector: Some("mmproj-Gemma4-26B-A4B-QAT-Uncensored-HauhauCS-Balanced-BF16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "gemma4-31b-uncensored",
        name: "Gemma 4 31B Uncensored",
        quantizations: &[QuantizationDefinition::new(
            "Q4_K_M",
            "Q4_K M",
            "Gemma4-31B-QAT-Uncensored-HauhauCS-Balanced-Q4_K_M.gguf",
        )],
        generation: ModelGeneration {
            temperature: Some(0.6),
            top_k: Some(64),
            top_p: Some(0.9),
            min_p: Some(0.05),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.1),
            frequency_penalty: None,
            presence_penalty: None,
        },
        repository: "HauhauCS/Gemma4-31B-QAT-Uncensored-HauhauCS-Balanced-MTP",
        projector: Some("mmproj-Gemma4-31B-QAT-Uncensored-HauhauCS-Balanced-BF16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.5-0.8b",
        name: "Qwen 3.5 0.8B",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "Qwen3.5-0.8B-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("IQ2_M", "IQ2 M", "Qwen3.5-0.8B-UD-IQ2_M.gguf"),
            QuantizationDefinition::new("IQ2_XXS", "IQ2 XXS", "Qwen3.5-0.8B-UD-IQ2_XXS.gguf"),
            QuantizationDefinition::new("IQ3_XXS", "IQ3 XXS", "Qwen3.5-0.8B-UD-IQ3_XXS.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "Qwen3.5-0.8B-UD-Q2_K_XL.gguf"),
            QuantizationDefinition::new("Q3_K_XL", "Q3_K XL", "Qwen3.5-0.8B-UD-Q3_K_XL.gguf"),
            QuantizationDefinition::new("Q5_K_XL", "Q5_K XL", "Qwen3.5-0.8B-UD-Q5_K_XL.gguf"),
            QuantizationDefinition::new("Q6_K_XL", "Q6_K XL", "Qwen3.5-0.8B-UD-Q6_K_XL.gguf"),
            QuantizationDefinition::new("Q8_K_XL", "Q8_K XL", "Qwen3.5-0.8B-UD-Q8_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(20),
            top_p: Some(1.0),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(2.0),
        },
        repository: "unsloth/Qwen3.5-0.8B-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.5-2b",
        name: "Qwen 3.5 2B",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "Qwen3.5-2B-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("IQ2_M", "IQ2 M", "Qwen3.5-2B-UD-IQ2_M.gguf"),
            QuantizationDefinition::new("IQ2_XXS", "IQ2 XXS", "Qwen3.5-2B-UD-IQ2_XXS.gguf"),
            QuantizationDefinition::new("IQ3_XXS", "IQ3 XXS", "Qwen3.5-2B-UD-IQ3_XXS.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "Qwen3.5-2B-UD-Q2_K_XL.gguf"),
            QuantizationDefinition::new("Q3_K_XL", "Q3_K XL", "Qwen3.5-2B-UD-Q3_K_XL.gguf"),
            QuantizationDefinition::new("Q5_K_XL", "Q5_K XL", "Qwen3.5-2B-UD-Q5_K_XL.gguf"),
            QuantizationDefinition::new("Q6_K_XL", "Q6_K XL", "Qwen3.5-2B-UD-Q6_K_XL.gguf"),
            QuantizationDefinition::new("Q8_K_XL", "Q8_K XL", "Qwen3.5-2B-UD-Q8_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(20),
            top_p: Some(1.0),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(2.0),
        },
        repository: "unsloth/Qwen3.5-2B-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.5-4b",
        name: "Qwen 3.5 4B",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "Qwen3.5-4B-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("IQ2_M", "IQ2 M", "Qwen3.5-4B-UD-IQ2_M.gguf"),
            QuantizationDefinition::new("IQ2_XXS", "IQ2 XXS", "Qwen3.5-4B-UD-IQ2_XXS.gguf"),
            QuantizationDefinition::new("IQ3_XXS", "IQ3 XXS", "Qwen3.5-4B-UD-IQ3_XXS.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "Qwen3.5-4B-UD-Q2_K_XL.gguf"),
            QuantizationDefinition::new("Q3_K_XL", "Q3_K XL", "Qwen3.5-4B-UD-Q3_K_XL.gguf"),
            QuantizationDefinition::new("Q5_K_XL", "Q5_K XL", "Qwen3.5-4B-UD-Q5_K_XL.gguf"),
            QuantizationDefinition::new("Q6_K_XL", "Q6_K XL", "Qwen3.5-4B-UD-Q6_K_XL.gguf"),
            QuantizationDefinition::new("Q8_K_XL", "Q8_K XL", "Qwen3.5-4B-UD-Q8_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(20),
            top_p: Some(1.0),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(2.0),
        },
        repository: "unsloth/Qwen3.5-4B-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.5-9b",
        name: "Qwen 3.5 9B",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "Qwen3.5-9B-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("IQ2_M", "IQ2 M", "Qwen3.5-9B-UD-IQ2_M.gguf"),
            QuantizationDefinition::new("IQ2_XXS", "IQ2 XXS", "Qwen3.5-9B-UD-IQ2_XXS.gguf"),
            QuantizationDefinition::new("IQ3_XXS", "IQ3 XXS", "Qwen3.5-9B-UD-IQ3_XXS.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "Qwen3.5-9B-UD-Q2_K_XL.gguf"),
            QuantizationDefinition::new("Q3_K_XL", "Q3_K XL", "Qwen3.5-9B-UD-Q3_K_XL.gguf"),
            QuantizationDefinition::new("Q5_K_XL", "Q5_K XL", "Qwen3.5-9B-UD-Q5_K_XL.gguf"),
            QuantizationDefinition::new("Q6_K_XL", "Q6_K XL", "Qwen3.5-9B-UD-Q6_K_XL.gguf"),
            QuantizationDefinition::new("Q8_K_XL", "Q8_K XL", "Qwen3.5-9B-UD-Q8_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(20),
            top_p: Some(1.0),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(2.0),
        },
        repository: "unsloth/Qwen3.5-9B-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.5-27b",
        name: "Qwen 3.5 27B",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "Qwen3.5-27B-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("IQ2_M", "IQ2 M", "Qwen3.5-27B-UD-IQ2_M.gguf"),
            QuantizationDefinition::new("IQ2_XXS", "IQ2 XXS", "Qwen3.5-27B-UD-IQ2_XXS.gguf"),
            QuantizationDefinition::new("IQ3_XXS", "IQ3 XXS", "Qwen3.5-27B-UD-IQ3_XXS.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "Qwen3.5-27B-UD-Q2_K_XL.gguf"),
            QuantizationDefinition::new("Q3_K_XL", "Q3_K XL", "Qwen3.5-27B-UD-Q3_K_XL.gguf"),
            QuantizationDefinition::new("Q5_K_XL", "Q5_K XL", "Qwen3.5-27B-UD-Q5_K_XL.gguf"),
            QuantizationDefinition::new("Q6_K_XL", "Q6_K XL", "Qwen3.5-27B-UD-Q6_K_XL.gguf"),
            QuantizationDefinition::new("Q8_K_XL", "Q8_K XL", "Qwen3.5-27B-UD-Q8_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(20),
            top_p: Some(1.0),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(2.0),
        },
        repository: "unsloth/Qwen3.5-27B-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.5-35b-a3b",
        name: "Qwen 3.5 35B A3B",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "Qwen3.5-35B-A3B-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("IQ2_M", "IQ2 M", "Qwen3.5-35B-A3B-UD-IQ2_M.gguf"),
            QuantizationDefinition::new("IQ2_XXS", "IQ2 XXS", "Qwen3.5-35B-A3B-UD-IQ2_XXS.gguf"),
            QuantizationDefinition::new("IQ3_XXS", "IQ3 XXS", "Qwen3.5-35B-A3B-UD-IQ3_XXS.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "Qwen3.5-35B-A3B-UD-Q2_K_XL.gguf"),
            QuantizationDefinition::new("Q3_K_XL", "Q3_K XL", "Qwen3.5-35B-A3B-UD-Q3_K_XL.gguf"),
            QuantizationDefinition::new("Q5_K_XL", "Q5_K XL", "Qwen3.5-35B-A3B-UD-Q5_K_XL.gguf"),
            QuantizationDefinition::new("Q6_K_XL", "Q6_K XL", "Qwen3.5-35B-A3B-UD-Q6_K_XL.gguf"),
            QuantizationDefinition::new("Q8_K_XL", "Q8_K XL", "Qwen3.5-35B-A3B-UD-Q8_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(20),
            top_p: Some(1.0),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(2.0),
        },
        repository: "unsloth/Qwen3.5-35B-A3B-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.6-27b",
        name: "Qwen 3.6 27B",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "Qwen3.6-27B-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("IQ2_M", "IQ2 M", "Qwen3.6-27B-UD-IQ2_M.gguf"),
            QuantizationDefinition::new("IQ2_XXS", "IQ2 XXS", "Qwen3.6-27B-UD-IQ2_XXS.gguf"),
            QuantizationDefinition::new("IQ3_XXS", "IQ3 XXS", "Qwen3.6-27B-UD-IQ3_XXS.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "Qwen3.6-27B-UD-Q2_K_XL.gguf"),
            QuantizationDefinition::new("Q3_K_XL", "Q3_K XL", "Qwen3.6-27B-UD-Q3_K_XL.gguf"),
            QuantizationDefinition::new("Q5_K_XL", "Q5_K XL", "Qwen3.6-27B-UD-Q5_K_XL.gguf"),
            QuantizationDefinition::new("Q6_K_XL", "Q6_K XL", "Qwen3.6-27B-UD-Q6_K_XL.gguf"),
            QuantizationDefinition::new("Q8_K_XL", "Q8_K XL", "Qwen3.6-27B-UD-Q8_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(0.7),
            top_k: Some(20),
            top_p: Some(0.8),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(1.5),
        },
        repository: "unsloth/Qwen3.6-27B-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.6-35b-a3b",
        name: "Qwen 3.6 35B A3B",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "Qwen3.6-35B-A3B-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("IQ1_M", "IQ1 M", "Qwen3.6-35B-A3B-UD-IQ1_M.gguf"),
            QuantizationDefinition::new("IQ2_M", "IQ2 M", "Qwen3.6-35B-A3B-UD-IQ2_M.gguf"),
            QuantizationDefinition::new("IQ2_XXS", "IQ2 XXS", "Qwen3.6-35B-A3B-UD-IQ2_XXS.gguf"),
            QuantizationDefinition::new("IQ3_S", "IQ3 S", "Qwen3.6-35B-A3B-UD-IQ3_S.gguf"),
            QuantizationDefinition::new("IQ3_XXS", "IQ3 XXS", "Qwen3.6-35B-A3B-UD-IQ3_XXS.gguf"),
            QuantizationDefinition::new("IQ4_NL", "IQ4 NL", "Qwen3.6-35B-A3B-UD-IQ4_NL.gguf"),
            QuantizationDefinition::new("IQ4_XS", "IQ4 XS", "Qwen3.6-35B-A3B-UD-IQ4_XS.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "Qwen3.6-35B-A3B-UD-Q2_K_XL.gguf"),
            QuantizationDefinition::new("Q3_K_M", "Q3_K M", "Qwen3.6-35B-A3B-UD-Q3_K_M.gguf"),
            QuantizationDefinition::new("Q3_K_S", "Q3_K S", "Qwen3.6-35B-A3B-UD-Q3_K_S.gguf"),
            QuantizationDefinition::new("Q3_K_XL", "Q3_K XL", "Qwen3.6-35B-A3B-UD-Q3_K_XL.gguf"),
            QuantizationDefinition::new("Q4_K_M", "Q4_K M", "Qwen3.6-35B-A3B-UD-Q4_K_M.gguf"),
            QuantizationDefinition::new("Q4_K_S", "Q4_K S", "Qwen3.6-35B-A3B-UD-Q4_K_S.gguf"),
            QuantizationDefinition::new("Q5_K_M", "Q5_K M", "Qwen3.6-35B-A3B-UD-Q5_K_M.gguf"),
            QuantizationDefinition::new("Q5_K_S", "Q5_K S", "Qwen3.6-35B-A3B-UD-Q5_K_S.gguf"),
            QuantizationDefinition::new("Q5_K_XL", "Q5_K XL", "Qwen3.6-35B-A3B-UD-Q5_K_XL.gguf"),
            QuantizationDefinition::new("Q6_K", "Q6_K", "Qwen3.6-35B-A3B-UD-Q6_K.gguf"),
            QuantizationDefinition::new("Q6_K_XL", "Q6_K XL", "Qwen3.6-35B-A3B-UD-Q6_K_XL.gguf"),
            QuantizationDefinition::new("Q8_K_XL", "Q8_K XL", "Qwen3.6-35B-A3B-UD-Q8_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(0.7),
            top_k: Some(20),
            top_p: Some(0.8),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(1.5),
        },
        repository: "unsloth/Qwen3.6-35B-A3B-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.8-27b",
        name: "Qwen 3.8 27B",
        quantizations: &[
            QuantizationDefinition::new("Q4_K_XL", "Q4_K XL", "Qwen3.8-27B-UD-Q4_K_XL.gguf"),
            QuantizationDefinition::new("IQ2_M", "IQ2 M", "Qwen3.8-27B-UD-IQ2_M.gguf"),
            QuantizationDefinition::new("IQ2_XXS", "IQ2 XXS", "Qwen3.8-27B-UD-IQ2_XXS.gguf"),
            QuantizationDefinition::new("IQ3_XXS", "IQ3 XXS", "Qwen3.8-27B-UD-IQ3_XXS.gguf"),
            QuantizationDefinition::new("Q2_K_XL", "Q2_K XL", "Qwen3.8-27B-UD-Q2_K_XL.gguf"),
            QuantizationDefinition::new("Q3_K_XL", "Q3_K XL", "Qwen3.8-27B-UD-Q3_K_XL.gguf"),
            QuantizationDefinition::new("Q5_K_XL", "Q5_K XL", "Qwen3.8-27B-UD-Q5_K_XL.gguf"),
            QuantizationDefinition::new("Q6_K_XL", "Q6_K XL", "Qwen3.8-27B-UD-Q6_K_XL.gguf"),
            QuantizationDefinition::new("Q8_K_XL", "Q8_K XL", "Qwen3.8-27B-UD-Q8_K_XL.gguf"),
        ],
        generation: ModelGeneration {
            temperature: Some(0.7),
            top_k: Some(20),
            top_p: Some(0.8),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(1.5),
        },
        repository: "unsloth/Qwen3.8-27B-GGUF",
        projector: Some("mmproj-F16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.5-2b-uncensored",
        name: "Qwen 3.5 2B Uncensored",
        quantizations: &[
            QuantizationDefinition::new(
                "Q4_K_M",
                "Q4_K M",
                "Qwen3.5-2B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
            ),
            QuantizationDefinition::new(
                "BF16",
                "BF16",
                "Qwen3.5-2B-Uncensored-HauhauCS-Aggressive-BF16.gguf",
            ),
            QuantizationDefinition::new(
                "Q6_K",
                "Q6_K",
                "Qwen3.5-2B-Uncensored-HauhauCS-Aggressive-Q6_K.gguf",
            ),
            QuantizationDefinition::new(
                "Q8_0",
                "Q8_0",
                "Qwen3.5-2B-Uncensored-HauhauCS-Aggressive-Q8_0.gguf",
            ),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(20),
            top_p: Some(1.0),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(2.0),
        },
        repository: "HauhauCS/Qwen3.5-2B-Uncensored-HauhauCS-Aggressive",
        projector: Some("mmproj-Qwen3.5-2B-Uncensored-HauhauCS-Aggressive-f16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.5-4b-uncensored",
        name: "Qwen 3.5 4B Uncensored",
        quantizations: &[
            QuantizationDefinition::new(
                "Q4_K_M",
                "Q4_K M",
                "Qwen3.5-4B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
            ),
            QuantizationDefinition::new(
                "BF16",
                "BF16",
                "Qwen3.5-4B-Uncensored-HauhauCS-Aggressive-BF16.gguf",
            ),
            QuantizationDefinition::new(
                "Q6_K",
                "Q6_K",
                "Qwen3.5-4B-Uncensored-HauhauCS-Aggressive-Q6_K.gguf",
            ),
            QuantizationDefinition::new(
                "Q8_0",
                "Q8_0",
                "Qwen3.5-4B-Uncensored-HauhauCS-Aggressive-Q8_0.gguf",
            ),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(20),
            top_p: Some(1.0),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(2.0),
        },
        repository: "HauhauCS/Qwen3.5-4B-Uncensored-HauhauCS-Aggressive",
        projector: Some("mmproj-Qwen3.5-4B-Uncensored-HauhauCS-Aggressive-BF16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.5-9b-uncensored",
        name: "Qwen 3.5 9B Uncensored",
        quantizations: &[
            QuantizationDefinition::new(
                "Q4_K_M",
                "Q4_K M",
                "Qwen3.5-9B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
            ),
            QuantizationDefinition::new(
                "BF16",
                "BF16",
                "Qwen3.5-9B-Uncensored-HauhauCS-Aggressive-BF16.gguf",
            ),
            QuantizationDefinition::new(
                "Q6_K",
                "Q6_K",
                "Qwen3.5-9B-Uncensored-HauhauCS-Aggressive-Q6_K.gguf",
            ),
            QuantizationDefinition::new(
                "Q8_0",
                "Q8_0",
                "Qwen3.5-9B-Uncensored-HauhauCS-Aggressive-Q8_0.gguf",
            ),
        ],
        generation: ModelGeneration {
            temperature: Some(1.0),
            top_k: Some(20),
            top_p: Some(1.0),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(2.0),
        },
        repository: "HauhauCS/Qwen3.5-9B-Uncensored-HauhauCS-Aggressive",
        projector: Some("mmproj-Qwen3.5-9B-Uncensored-HauhauCS-Aggressive-BF16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.6-27b-uncensored",
        name: "Qwen 3.6 27B Uncensored",
        quantizations: &[
            QuantizationDefinition::new(
                "Q4_K_P",
                "Q4_K P",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-Q4_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "IQ2_M",
                "IQ2 M",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-IQ2_M.gguf",
            ),
            QuantizationDefinition::new(
                "IQ3_M",
                "IQ3 M",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-IQ3_M.gguf",
            ),
            QuantizationDefinition::new(
                "IQ3_XS",
                "IQ3 XS",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-IQ3_XS.gguf",
            ),
            QuantizationDefinition::new(
                "IQ4_XS",
                "IQ4 XS",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-IQ4_XS.gguf",
            ),
            QuantizationDefinition::new(
                "Q2_K_P",
                "Q2_K P",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-Q2_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q3_K_P",
                "Q3_K P",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-Q3_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q5_K_P",
                "Q5_K P",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-Q5_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q6_K_P",
                "Q6_K P",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-Q6_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q8_K_P",
                "Q8_K P",
                "Qwen3.6-27B-Uncensored-HauhauCS-Balanced-Q8_K_P.gguf",
            ),
        ],
        generation: ModelGeneration {
            temperature: Some(0.7),
            top_k: Some(20),
            top_p: Some(0.8),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(1.5),
        },
        repository: "HauhauCS/Qwen3.6-27B-Uncensored-HauhauCS-Balanced",
        projector: Some("mmproj-Qwen3.6-27B-Uncensored-HauhauCS-Balanced-f16.gguf"),
        target_languages: SupportedLanguages::All,
    },
    LocalModelDescriptor {
        id: "qwen3.6-35b-a3b-uncensored",
        name: "Qwen 3.6 35B A3B Uncensored",
        quantizations: &[
            QuantizationDefinition::new(
                "Q4_K_P",
                "Q4_K P",
                "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-Q4_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "IQ2_M",
                "IQ2 M",
                "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-IQ2_M.gguf",
            ),
            QuantizationDefinition::new(
                "IQ3_M",
                "IQ3 M",
                "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-IQ3_M.gguf",
            ),
            QuantizationDefinition::new(
                "IQ4_XS",
                "IQ4 XS",
                "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-IQ4_XS.gguf",
            ),
            QuantizationDefinition::new(
                "Q2_K_P",
                "Q2_K P",
                "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-Q2_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q3_K_P",
                "Q3_K P",
                "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-Q3_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q5_K_P",
                "Q5_K P",
                "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-Q5_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q6_K_P",
                "Q6_K P",
                "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-Q6_K_P.gguf",
            ),
            QuantizationDefinition::new(
                "Q8_K_P",
                "Q8_K P",
                "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-Q8_K_P.gguf",
            ),
        ],
        generation: ModelGeneration {
            temperature: Some(0.7),
            top_k: Some(20),
            top_p: Some(0.8),
            min_p: Some(0.0),
            max_tokens: Some(1000),
            repeat_penalty: Some(1.0),
            frequency_penalty: None,
            presence_penalty: Some(1.5),
        },
        repository: "HauhauCS/Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive",
        projector: Some("mmproj-Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-f16.gguf"),
        target_languages: SupportedLanguages::All,
    },
];

pub(crate) struct ResolvedLocalModel {
    pub(crate) model: PathBuf,
    pub(crate) projector: Option<PathBuf>,
}

impl LocalModelDescriptor {
    pub(crate) async fn resolve(
        self,
        selection: &ModelSelection,
    ) -> anyhow::Result<ResolvedLocalModel> {
        let filename = self.filename(selection.quantization.as_deref())?;
        let model = koharu_runtime::HuggingFaceFile::latest(self.repository, filename).resolve();
        let projector = async {
            match self.projector {
                Some(filename) => Ok(Some(
                    koharu_runtime::HuggingFaceFile::latest(self.repository, filename)
                        .resolve()
                        .await?,
                )),
                None => Ok(None),
            }
        };
        let (model, projector) = tokio::try_join!(model, projector)?;
        Ok(ResolvedLocalModel { model, projector })
    }

    fn filename(self, quantization: Option<&str>) -> anyhow::Result<&'static str> {
        let quantization = quantization.or_else(|| {
            self.quantizations
                .first()
                .map(|quantization| quantization.id)
        });
        let quantization = quantization
            .ok_or_else(|| anyhow::anyhow!("local model '{}' has no quantizations", self.id))?;
        let definition = self
            .quantizations
            .iter()
            .find(|definition| definition.id == quantization)
            .ok_or_else(|| {
                anyhow::anyhow!("unknown quantization '{quantization}' for '{}'", self.id)
            })?;
        Ok(definition.filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_catalog_has_unique_complete_entries() {
        assert_eq!(MODELS.len(), 26);
        for (index, model) in MODELS.iter().enumerate() {
            let id = model.id;
            assert!(!model.repository.is_empty());
            assert!(!model.quantizations.is_empty());
            if let Some(projector) = model.projector {
                assert!(!projector.is_empty());
            }
            for (quantization_index, quantization) in model.quantizations.iter().enumerate() {
                assert!(!quantization.filename.is_empty());
                assert!(
                    !model.quantizations[..quantization_index]
                        .iter()
                        .any(|other| other.id == quantization.id)
                );
            }
            assert!(!MODELS[..index].iter().any(|other| other.id == id));
        }
    }

    #[test]
    fn quantization_selects_its_exact_hugging_face_filename() {
        let descriptor = MODELS
            .iter()
            .find(|descriptor| descriptor.id == "qwen3.5-2b")
            .expect("Qwen model is cataloged");
        assert_eq!(
            descriptor
                .filename(Some("Q8_K_XL"))
                .expect("quantization is valid"),
            "Qwen3.5-2B-UD-Q8_K_XL.gguf"
        );
    }
}
