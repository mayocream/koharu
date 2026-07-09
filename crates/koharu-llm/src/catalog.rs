use std::{fmt, path::PathBuf};

use koharu_runtime::package::{Package, huggingface::HuggingFace};
use strum::{EnumProperty, IntoEnumIterator};

use crate::GenerationOptions;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum ModelSource {
    Builtin(BuiltinModel),
    HuggingFace { repo: String, filename: String },
    Path(PathBuf),
}

impl ModelSource {
    pub fn builtin(model: BuiltinModel) -> Self {
        Self::Builtin(model)
    }

    pub fn huggingface(repo: impl Into<String>, filename: impl Into<String>) -> Self {
        Self::HuggingFace {
            repo: repo.into(),
            filename: filename.into(),
        }
    }

    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self::Path(path.into())
    }

    pub async fn resolve(&self) -> anyhow::Result<PathBuf> {
        match self {
            Self::Builtin(model) => model.resolve().await,
            Self::HuggingFace { repo, filename } => {
                HuggingFace {
                    repo: repo.clone(),
                    filename: filename.clone(),
                }
                .resolve()
                .await
            }
            Self::Path(path) => Ok(path.clone()),
        }
    }

    pub fn default_generation_options(&self) -> GenerationOptions {
        match self {
            Self::Builtin(model) => model.default_generation_options(),
            Self::HuggingFace { .. } | Self::Path(_) => GenerationOptions::default(),
        }
    }
}

impl From<BuiltinModel> for ModelSource {
    fn from(value: BuiltinModel) -> Self {
        Self::Builtin(value)
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::Display,
    strum::EnumString,
    strum::EnumIter,
    strum::EnumProperty,
    serde::Deserialize,
    serde::Serialize,
)]
pub enum BuiltinModel {
    #[strum(
        serialize = "vntl-llama3-8b-v2",
        props(
            repo = "lmg-anon/vntl-llama3-8b-v2-gguf",
            filename = "vntl-llama3-8b-v2-hf-q5_k_m.gguf",
            languages = "en-US"
        )
    )]
    VntlLlama3_8Bv2,
    #[strum(
        serialize = "lfm2.5-1.2b-instruct",
        props(
            repo = "LiquidAI/LFM2.5-1.2B-Instruct-GGUF",
            filename = "LFM2.5-1.2B-Instruct-Q4_K_M.gguf",
            languages = "en-US,ar-SA,zh-CN,fr-FR,de-DE,ja-JP,ko-KR,pt-PT,es-ES"
        )
    )]
    Lfm2_5_1_2bInstruct,
    #[strum(
        serialize = "sakura-galtransl-7b-v3.7",
        props(
            repo = "SakuraLLM/Sakura-GalTransl-7B-v3.7",
            filename = "Sakura-Galtransl-7B-v3.7-IQ4_XS.gguf",
            languages = "zh-CN"
        )
    )]
    SakuraGalTransl7Bv3_7,
    #[strum(
        serialize = "sakura-1.5b-qwen2.5-v1.0",
        props(
            repo = "shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX",
            filename = "sakura-1.5b-qwen2.5-v1.0-Q5KS.gguf",
            languages = "zh-CN"
        )
    )]
    Sakura1_5bQwen2_5v1_0,
    #[strum(
        serialize = "hunyuan-mt-7b",
        props(
            repo = "Mungert/Hunyuan-MT-7B-GGUF",
            filename = "Hunyuan-MT-7B-q4_k_m.gguf",
            languages = "zh-CN,en-US,fr-FR,pt-PT,pt-BR,es-ES,ja-JP,tr-TR,ru-RU,ar-SA,ko-KR,th-TH,it-IT,de-DE,vi-VN,ms-MY,id-ID,fil-PH,hi-IN,zh-TW,pl-PL,cs-CZ,nl-NL,km-KH,my-MM,fa-IR,gu-IN,ur-PK,te-IN,mr-IN,he-IL,bn-BD,ta-IN,uk-UA,bo-CN,kk-KZ,mn-MN,ug-CN,yue-HK"
        )
    )]
    HunyuanMT7B,
    #[strum(
        serialize = "sugoi-14b-ultra",
        props(
            repo = "sugoitoolkit/Sugoi-14B-Ultra-GGUF",
            filename = "Sugoi-14B-Ultra-Q4_K_M.gguf",
            languages = "en-US"
        )
    )]
    Sugoi14bUltra,
    #[strum(
        serialize = "sugoi-32b-ultra",
        props(
            repo = "sugoitoolkit/Sugoi-32B-Ultra-GGUF",
            filename = "Sugoi-32B-Ultra-Q4_K_M.gguf",
            languages = "en-US"
        )
    )]
    Sugoi32bUltra,
    #[strum(
        serialize = "gemma4-e2b-it",
        props(
            repo = "unsloth/gemma-4-E2B-it-GGUF",
            filename = "gemma-4-E2B-it-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Gemma4E2bIt,
    #[strum(
        serialize = "gemma4-e4b-it",
        props(
            repo = "unsloth/gemma-4-E4B-it-GGUF",
            filename = "gemma-4-E4B-it-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Gemma4E4bIt,
    #[strum(
        serialize = "gemma4-12b-it",
        props(
            repo = "unsloth/gemma-4-12b-it-GGUF",
            filename = "gemma-4-12b-it-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Gemma4_12bIt,
    #[strum(
        serialize = "gemma4-26b-a4b-it",
        props(
            repo = "unsloth/gemma-4-26B-A4B-it-GGUF",
            filename = "gemma-4-26B-A4B-it-UD-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Gemma4_26bA4bIt,
    #[strum(
        serialize = "gemma4-31b-it",
        props(
            repo = "unsloth/gemma-4-31B-it-GGUF",
            filename = "gemma-4-31B-it-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Gemma4_31bIt,
    #[strum(
        serialize = "gemma4-e2b-uncensored",
        props(
            repo = "HauhauCS/Gemma-4-E2B-Uncensored-HauhauCS-Aggressive",
            filename = "Gemma-4-E2B-Uncensored-HauhauCS-Aggressive-Q4_K_P.gguf",
            languages = "*"
        )
    )]
    Gemma4E2bUncensored,
    #[strum(
        serialize = "gemma4-e4b-uncensored",
        props(
            repo = "HauhauCS/Gemma-4-E4B-Uncensored-HauhauCS-Aggressive",
            filename = "Gemma-4-E4B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Gemma4E4bUncensored,
    #[strum(
        serialize = "qwen3.5-0.8b",
        props(
            repo = "unsloth/Qwen3.5-0.8B-GGUF",
            filename = "Qwen3.5-0.8B-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_0_8b,
    #[strum(
        serialize = "qwen3.5-2b",
        props(
            repo = "unsloth/Qwen3.5-2B-GGUF",
            filename = "Qwen3.5-2B-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_2b,
    #[strum(
        serialize = "qwen3.5-4b",
        props(
            repo = "unsloth/Qwen3.5-4B-GGUF",
            filename = "Qwen3.5-4B-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_4b,
    #[strum(
        serialize = "qwen3.5-9b",
        props(
            repo = "unsloth/Qwen3.5-9B-GGUF",
            filename = "Qwen3.5-9B-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_9b,
    #[strum(
        serialize = "qwen3.5-27b",
        props(
            repo = "unsloth/Qwen3.5-27B-GGUF",
            filename = "Qwen3.5-27B-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_27b,
    #[strum(
        serialize = "qwen3.5-35b-a3b",
        props(
            repo = "unsloth/Qwen3.5-35B-A3B-GGUF",
            filename = "Qwen3.5-35B-A3B-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_35bA3b,
    #[strum(
        serialize = "qwen3.6-27b",
        props(
            repo = "unsloth/Qwen3.6-27B-GGUF",
            filename = "Qwen3.6-27B-IQ4_XS.gguf",
            languages = "*"
        )
    )]
    Qwen3_6_27b,
    #[strum(
        serialize = "qwen3.6-35b-a3b",
        props(
            repo = "unsloth/Qwen3.6-35B-A3B-GGUF",
            filename = "Qwen3.6-35B-A3B-UD-IQ4_XS.gguf",
            languages = "*"
        )
    )]
    Qwen3_6_35bA3b,
    #[strum(
        serialize = "qwen3.5-2b-uncensored",
        props(
            repo = "HauhauCS/Qwen3.5-2B-Uncensored-HauhauCS-Aggressive",
            filename = "Qwen3.5-2B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_2bUncensored,
    #[strum(
        serialize = "qwen3.5-4b-uncensored",
        props(
            repo = "HauhauCS/Qwen3.5-4B-Uncensored-HauhauCS-Aggressive",
            filename = "Qwen3.5-4B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_4bUncensored,
    #[strum(
        serialize = "qwen3.5-9b-uncensored",
        props(
            repo = "HauhauCS/Qwen3.5-9B-Uncensored-HauhauCS-Aggressive",
            filename = "Qwen3.5-9B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_9bUncensored,
    #[strum(
        serialize = "qwen3.5-27b-uncensored",
        props(
            repo = "HauhauCS/Qwen3.5-27B-Uncensored-HauhauCS-Aggressive",
            filename = "Qwen3.5-27B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_27bUncensored,
    #[strum(
        serialize = "qwen3.5-35b-a3b-uncensored",
        props(
            repo = "HauhauCS/Qwen3.5-35B-A3B-Uncensored-HauhauCS-Aggressive",
            filename = "Qwen3.5-35B-A3B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf",
            languages = "*"
        )
    )]
    Qwen3_5_35bA3bUncensored,
    #[strum(
        serialize = "qwen3.6-27b-uncensored",
        props(
            repo = "HauhauCS/Qwen3.6-27B-Uncensored-HauhauCS-Aggressive",
            filename = "Qwen3.6-27B-Uncensored-HauhauCS-Aggressive-IQ4_XS.gguf",
            languages = "*"
        )
    )]
    Qwen3_6_27bUncensored,
    #[strum(
        serialize = "qwen3.6-35b-a3b-uncensored",
        props(
            repo = "HauhauCS/Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive",
            filename = "Qwen3.6-35B-A3B-Uncensored-HauhauCS-Aggressive-IQ4_XS.gguf",
            languages = "*"
        )
    )]
    Qwen3_6_35bA3bUncensored,
}

impl BuiltinModel {
    pub fn all() -> impl Iterator<Item = Self> {
        Self::iter()
    }

    pub fn repo(self) -> &'static str {
        self.property("repo")
    }

    pub fn filename(self) -> &'static str {
        self.property("filename")
    }

    pub fn languages(self) -> &'static str {
        self.property("languages")
    }

    pub async fn resolve(self) -> anyhow::Result<PathBuf> {
        HuggingFace {
            repo: self.repo().to_owned(),
            filename: self.filename().to_owned(),
        }
        .resolve()
        .await
    }

    pub fn default_generation_options(self) -> GenerationOptions {
        match self {
            Self::Lfm2_5_1_2bInstruct => GenerationOptions {
                temperature: 0.1,
                top_k: Some(50),
                repeat_penalty: 1.05,
                ..Default::default()
            },
            Self::Gemma4E2bIt
            | Self::Gemma4E4bIt
            | Self::Gemma4_12bIt
            | Self::Gemma4_26bA4bIt
            | Self::Gemma4_31bIt
            | Self::Gemma4E2bUncensored
            | Self::Gemma4E4bUncensored => GenerationOptions {
                temperature: 1.0,
                top_k: Some(64),
                top_p: Some(0.95),
                repeat_penalty: 1.0,
                ..Default::default()
            },
            Self::Qwen3_5_0_8b
            | Self::Qwen3_5_2b
            | Self::Qwen3_5_4b
            | Self::Qwen3_5_9b
            | Self::Qwen3_5_27b
            | Self::Qwen3_5_35bA3b
            | Self::Qwen3_5_2bUncensored
            | Self::Qwen3_5_4bUncensored
            | Self::Qwen3_5_9bUncensored
            | Self::Qwen3_5_27bUncensored
            | Self::Qwen3_5_35bA3bUncensored => GenerationOptions {
                temperature: 1.0,
                top_k: Some(20),
                top_p: Some(1.0),
                min_p: Some(0.0),
                presence_penalty: 2.0,
                repeat_penalty: 1.0,
                ..Default::default()
            },
            Self::Qwen3_6_27b
            | Self::Qwen3_6_35bA3b
            | Self::Qwen3_6_27bUncensored
            | Self::Qwen3_6_35bA3bUncensored => GenerationOptions {
                temperature: 0.7,
                top_k: Some(20),
                top_p: Some(0.8),
                min_p: Some(0.0),
                presence_penalty: 1.5,
                repeat_penalty: 1.0,
                ..Default::default()
            },
            Self::Sugoi14bUltra | Self::Sugoi32bUltra => GenerationOptions {
                temperature: 0.1,
                top_k: Some(40),
                top_p: Some(0.95),
                min_p: Some(0.05),
                repeat_penalty: 1.1,
                ..Default::default()
            },
            _ => GenerationOptions::default(),
        }
    }

    fn property(self, name: &str) -> &'static str {
        self.get_str(name).expect("missing built-in model property")
    }
}

impl fmt::Display for ModelSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(model) => write!(f, "{model}"),
            Self::HuggingFace { repo, filename } => write!(f, "{repo}/{filename}"),
            Self::Path(path) => write!(f, "{}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BuiltinModel;

    #[test]
    fn builtins_have_repo_and_filename() {
        for model in BuiltinModel::all() {
            assert!(!model.repo().is_empty());
            assert!(model.filename().ends_with(".gguf"));
        }
    }
}
