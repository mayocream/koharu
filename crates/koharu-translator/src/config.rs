use anyhow::Result;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    CaiyunConfig, ClaudeConfig, DeepLConfig, DeepSeekConfig, GeminiConfig, GoogleCloudConfig,
    LmStudioConfig, LocalConfig, OpenAiCompatibleConfig, OpenAiConfig, OpenRouterConfig,
};

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct TranslationConfig {
    pub model: Providers,
    pub target_language: String,
    pub instructions: Option<String>,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            model: Providers::Local(LocalConfig::default()),
            target_language: "en-US".into(),
            instructions: None,
        }
    }
}

impl TranslationConfig {
    pub fn load() -> Result<koharu_config::Config<Self>> {
        koharu_config::load("translation")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum Providers {
    Local(LocalConfig),
    #[serde(rename = "openai")]
    OpenAi(OpenAiConfig),
    Gemini(GeminiConfig),
    Claude(ClaudeConfig),
    #[serde(rename = "deepseek")]
    DeepSeek(DeepSeekConfig),
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible(OpenAiCompatibleConfig),
    #[serde(rename = "openrouter")]
    OpenRouter(OpenRouterConfig),
    #[serde(rename = "lm_studio")]
    LmStudio(LmStudioConfig),
    #[serde(rename = "deepl")]
    DeepL(DeepLConfig),
    GoogleCloudTranslation(GoogleCloudConfig),
    Caiyun(CaiyunConfig),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_gemma_4_12b() {
        let Providers::Local(config) = TranslationConfig::default().model else {
            panic!("expected local translation configuration");
        };
        assert_eq!(config.model, "gemma4-12b-it");
    }

    #[test]
    fn parses_translation_config() {
        let config: TranslationConfig = toml::from_str(
            r#"
                target_language = "en-US"

                [model]
                provider = "openai"
                model = "gpt-4.1-mini"
                thinking = false
            "#,
        )
        .unwrap();

        let Providers::OpenAi(model) = config.model else {
            panic!("expected OpenAI configuration");
        };
        assert!(!model.thinking);
    }

    #[test]
    fn serialized_config_has_no_credentials() {
        let document = toml::to_string(&TranslationConfig::default()).unwrap();
        assert!(!document.contains("credentials"));
        assert!(!document.contains("openai ="));
    }

    #[test]
    fn serializes_openrouter_provider_name() {
        let value =
            serde_json::to_value(Providers::OpenRouter(OpenRouterConfig::default())).unwrap();
        assert_eq!(value["provider"], "openrouter");
    }

    #[test]
    fn serializes_lm_studio_provider_name() {
        let value = serde_json::to_value(Providers::LmStudio(LmStudioConfig::default())).unwrap();
        assert_eq!(value["provider"], "lm_studio");
    }
}
