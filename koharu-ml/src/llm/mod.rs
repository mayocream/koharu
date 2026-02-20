pub mod facade;
mod model;
pub mod prompt;
mod quantized_hunyuan_dense;
mod quantized_lfm2;
mod tokenizer;

pub use model::{GenerateOptions, Llm};
pub use prompt::{ChatMessage, ChatRole};

macro_rules! define_languages {
    ( $( $code:literal => $name:literal ),* $(,)? ) => {
        pub const SUPPORTED_LANGUAGES: &[(&str, &str)] = &[
            $(($code, $name)),*
        ];

        pub fn supported_locales() -> Vec<String> {
            SUPPORTED_LANGUAGES
                .iter()
                .map(|(_, name)| (*name).to_string())
                .collect()
        }

        pub fn language_from_tag(lang: &str) -> &'static str {
            SUPPORTED_LANGUAGES
                .iter()
                .find(|(code, _)| *code == lang)
                .map(|(_, name)| *name)
                .unwrap_or("English")
        }

        fn map_language_codes(codes: &[&str]) -> Vec<String> {
            codes
                .iter()
                .map(|code| language_from_tag(code).to_string())
                .collect()
        }
    };
}

macro_rules! define_llms {
    (
        $(
            $name:ident => {
                id = $id:literal,
                repo = $repo:literal,
                filename = $weights:literal
                $(, languages = [$($lang:literal),* $(,)?])?
            }
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString, strum::EnumIter)]
        pub enum ModelId {
            $(
                #[strum(serialize = $id)]
                $name,
            )*
        }

        $(
            #[allow(non_snake_case)]
            mod $name {
                use crate::define_models;

                define_models! {
                    Model => ($repo, $weights)
                }
            }
        )*

        impl ModelId {
            pub async fn get(&self) -> anyhow::Result<std::path::PathBuf> {
                match self {
                    $(
                        ModelId::$name => {
                            let weights = $name::Manifest::Model.get().await?;
                            Ok(weights)
                        }
                    ),*
                }
            }

            pub fn languages(&self) -> Vec<String> {
                match self {
                    $(ModelId::$name => map_language_codes(&[$($($lang),*)?]),)*
                }
            }
        }

        pub async fn prefetch() -> anyhow::Result<()> {
            use futures::stream::{self, StreamExt, TryStreamExt};
            let models = [
                $(ModelId::$name),*
            ];
            stream::iter(models)
                .map(|manifest| async move {
                    manifest.get().await
                })
                .buffer_unordered(num_cpus::get())
                .try_collect::<Vec<_>>()
                .await?;
            Ok(())
        }
    };
}

define_languages! {
    "zh" => "简体中文",
    "en" => "English",
    "fr" => "Français",
    "pt" => "Português",
    "es" => "Español",
    "ja" => "日本語",
    "tr" => "Türkçe",
    "ru" => "Русский",
    "ar" => "العربية",
    "ko" => "한국어",
    "th" => "ไทย",
    "it" => "Italiano",
    "de" => "Deutsch",
    "vi" => "Tiếng Việt",
    "ms" => "Bahasa Melayu",
    "id" => "Bahasa Indonesia",
    "tl" => "Filipino",
    "hi" => "हिन्दी",
    "zh-Hant" => "繁體中文",
    "pl" => "Polski",
    "cs" => "Čeština",
    "nl" => "Nederlands",
    "km" => "ភាសាខ្មែរ",
    "my" => "မြန်မာ",
    "fa" => "فارسی",
    "gu" => "ગુજરાતી",
    "ur" => "اردو",
    "te" => "తెలుగు",
    "mr" => "मराठी",
    "he" => "עברית",
    "bn" => "বাংলা",
    "ta" => "தமிழ்",
    "uk" => "Українська",
    "bo" => "བོད་ཡིག",
    "kk" => "Қазақ тілі",
    "mn" => "Монгол",
    "ug" => "ئۇيغۇرچە",
    "yue" => "粵語",
}

define_llms! {
    VntlLlama3_8Bv2 => {
        id = "vntl-llama3-8b-v2",
        repo = "lmg-anon/vntl-llama3-8b-v2-gguf",
        filename = "vntl-llama3-8b-v2-hf-q8_0.gguf",
        languages = ["en"]
    },
    Lfm2_350mEnjpMt => {
        id = "lfm2-350m-enjp-mt",
        repo = "LiquidAI/LFM2-350M-ENJP-MT-GGUF",
        filename = "LFM2-350M-ENJP-MT-Q8_0.gguf",
        languages = ["en"]
    },
    SakuraGalTransl7Bv3_7 => {
        id = "sakura-galtransl-7b-v3.7",
        repo = "SakuraLLM/Sakura-GalTransl-7B-v3.7",
        filename = "Sakura-Galtransl-7B-v3.7.gguf",
        languages = ["zh"]
    },
    Sakura1_5bQwen2_5v1_0 => {
        id = "sakura-1.5b-qwen2.5-v1.0",
        repo = "shing3232/Sakura-1.5B-Qwen2.5-v1.0-GGUF-IMX",
        filename = "sakura-1.5b-qwen2.5-v1.0-Q5KS.gguf",
        languages = ["zh"]
    },
    // Mungert/Hunyuan-MT-7B-GGUF
    HunyuanMT7B => {
        id = "hunyuan-mt-7b",
        repo = "Mungert/Hunyuan-MT-7B-GGUF",
        filename = "Hunyuan-MT-7B-q6_k_m.gguf",
        languages = [
            "zh", "en", "fr", "pt", "es", "ja", "tr", "ru", "ar",
            "ko", "th", "it", "de", "vi", "ms", "id", "tl", "hi",
            "zh-Hant", "pl", "cs", "nl", "km", "my", "fa", "gu",
            "ur", "te", "mr", "he", "bn", "ta", "uk", "bo", "kk",
            "mn", "ug", "yue",
        ]
    },
}
