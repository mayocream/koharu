mod caiyun;
mod claude;
mod deepl;
mod deepseek;
mod gemini;
mod google_cloud;
mod lm_studio;
mod openai;
mod openai_compatible;
mod openrouter;

use anyhow::Context;
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;

pub use caiyun::CaiyunConfig;
pub use claude::ClaudeConfig;
pub use deepl::DeepLConfig;
pub use deepseek::DeepSeekConfig;
pub use gemini::GeminiConfig;
pub use google_cloud::GoogleCloudConfig;
pub use lm_studio::{LmStudioConfig, discover_lm_studio_models};
pub use openai::OpenAiConfig;
pub use openai_compatible::{OpenAiCompatibleConfig, discover_openai_compatible_models};
pub use openrouter::{OpenRouterConfig, discover_openrouter_models};

use crate::{Error, RemoteProviderKind, Result, Translation, TranslationRequest, Translator};

#[derive(Debug, Clone)]
pub enum RemoteProvider {
    OpenAi(OpenAiConfig),
    Gemini(GeminiConfig),
    Claude(ClaudeConfig),
    DeepSeek(DeepSeekConfig),
    OpenAiCompatible(OpenAiCompatibleConfig),
    OpenRouter(OpenRouterConfig),
    LmStudio(LmStudioConfig),
    DeepL(DeepLConfig),
    GoogleCloudTranslation(GoogleCloudConfig),
    Caiyun(CaiyunConfig),
}

impl RemoteProvider {
    #[must_use]
    pub const fn kind(&self) -> RemoteProviderKind {
        match self {
            Self::OpenAi(_) => RemoteProviderKind::OpenAi,
            Self::Gemini(_) => RemoteProviderKind::Gemini,
            Self::Claude(_) => RemoteProviderKind::Claude,
            Self::DeepSeek(_) => RemoteProviderKind::DeepSeek,
            Self::OpenAiCompatible(_) => RemoteProviderKind::OpenAiCompatible,
            Self::OpenRouter(_) => RemoteProviderKind::OpenRouter,
            Self::LmStudio(_) => RemoteProviderKind::LmStudio,
            Self::DeepL(_) => RemoteProviderKind::DeepL,
            Self::GoogleCloudTranslation(_) => RemoteProviderKind::GoogleCloudTranslation,
            Self::Caiyun(_) => RemoteProviderKind::Caiyun,
        }
    }
}

/// Optional generation controls used by hosted chat models.
#[derive(Debug, Clone, Copy, Default)]
pub struct RemoteGenerationOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: bool,
}

#[derive(Debug, Clone)]
pub struct RemoteTranslator {
    client: Client,
    provider: RemoteProvider,
}

impl RemoteTranslator {
    #[must_use]
    pub fn new(provider: RemoteProvider) -> Self {
        Self::with_client(Client::new(), provider)
    }

    #[must_use]
    pub fn with_client(client: Client, provider: RemoteProvider) -> Self {
        Self { client, provider }
    }

    #[must_use]
    pub fn with_generation_options(mut self, options: RemoteGenerationOptions) -> Self {
        match &mut self.provider {
            RemoteProvider::OpenAi(config) => {
                config.temperature = options.temperature;
                config.max_tokens = options.max_tokens;
                config.thinking = options.thinking;
            }
            RemoteProvider::Gemini(config) => {
                config.temperature = options.temperature;
                config.max_tokens = options.max_tokens;
                config.thinking = options.thinking;
            }
            RemoteProvider::Claude(config) => {
                config.temperature = options.temperature;
                config.max_tokens = options.max_tokens;
                config.thinking = options.thinking;
            }
            RemoteProvider::DeepSeek(config) => {
                config.temperature = options.temperature;
                config.max_tokens = options.max_tokens;
                config.thinking = options.thinking;
            }
            RemoteProvider::OpenAiCompatible(config) => {
                config.temperature = options.temperature;
                config.max_tokens = options.max_tokens;
            }
            RemoteProvider::OpenRouter(config) => {
                config.temperature = options.temperature;
                config.max_tokens = options.max_tokens;
                config.thinking = options.thinking;
            }
            RemoteProvider::LmStudio(config) => {
                config.temperature = options.temperature;
                config.max_tokens = options.max_tokens;
                config.thinking = options.thinking;
            }
            RemoteProvider::DeepL(_)
            | RemoteProvider::GoogleCloudTranslation(_)
            | RemoteProvider::Caiyun(_) => {}
        }
        self
    }

    #[must_use]
    pub fn configuration(&self) -> &RemoteProvider {
        &self.provider
    }
}

#[async_trait]
impl Translator for RemoteTranslator {
    fn provider(&self) -> &'static str {
        self.provider.kind().id()
    }

    async fn translate(&self, request: TranslationRequest) -> Result<Translation> {
        if request.segments.is_empty() {
            return Ok(Translation {
                segments: Vec::new(),
            });
        }

        let expected = request.segments.len();
        let segments = match &self.provider {
            RemoteProvider::OpenAi(config) => {
                openai::translate(&self.client, config, &request).await?
            }
            RemoteProvider::Gemini(config) => {
                gemini::translate(&self.client, config, &request).await?
            }
            RemoteProvider::Claude(config) => {
                claude::translate(&self.client, config, &request).await?
            }
            RemoteProvider::DeepSeek(config) => {
                deepseek::translate(&self.client, config, &request).await?
            }
            RemoteProvider::OpenAiCompatible(config) => {
                openai_compatible::compatible(&self.client, config, &request).await?
            }
            RemoteProvider::OpenRouter(config) => {
                openrouter::translate(&self.client, config, &request).await?
            }
            RemoteProvider::LmStudio(config) => {
                lm_studio::translate(&self.client, config, &request).await?
            }
            RemoteProvider::DeepL(config) => {
                deepl::translate(&self.client, config, &request).await?
            }
            RemoteProvider::GoogleCloudTranslation(config) => {
                google_cloud::translate(&self.client, config, &request).await?
            }
            RemoteProvider::Caiyun(config) => {
                caiyun::translate(&self.client, config, &request).await?
            }
        };

        if segments.len() != expected {
            return Err(Error::SegmentCount {
                provider: self.provider(),
                expected,
                actual: segments.len(),
            });
        }
        Ok(Translation { segments })
    }
}

pub(super) async fn send_json<T: DeserializeOwned>(
    provider: &'static str,
    request: RequestBuilder,
) -> Result<T> {
    let response = request
        .send()
        .await
        .with_context(|| format!("{provider} request failed"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .with_context(|| format!("failed to read {provider} response"))?;

    if !status.is_success() {
        let lower = text.to_ascii_lowercase();
        if status == StatusCode::TOO_MANY_REQUESTS
            || lower.contains("insufficient_quota")
            || lower.contains("resource_exhausted")
            || lower.contains("rate limit exceeded")
            || lower.contains("credit balance is too low")
        {
            return Err(Error::QuotaExceeded { provider });
        }
        return Err(Error::Api {
            provider,
            status: status.as_u16(),
            message: text.chars().take(4096).collect(),
        });
    }

    serde_json::from_str(&text)
        .with_context(|| format!("failed to decode {provider} response"))
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_are_credential_keys() {
        assert_eq!(RemoteProviderKind::OpenAi.id(), "openai");
        assert_eq!(RemoteProviderKind::OpenRouter.id(), "openrouter");
        assert_eq!(RemoteProviderKind::LmStudio.id(), "lm-studio");
        assert_eq!(
            RemoteProviderKind::GoogleCloudTranslation.id(),
            "google-cloud-translation"
        );
    }

    #[test]
    fn thinking_is_disabled_by_default() {
        assert!(!OpenAiConfig::default().thinking);
        assert!(!GeminiConfig::default().thinking);
        assert!(!ClaudeConfig::default().thinking);
        assert!(!DeepSeekConfig::default().thinking);
        assert!(!OpenRouterConfig::default().thinking);
        assert!(!LmStudioConfig::default().thinking);
        assert!(!RemoteGenerationOptions::default().thinking);
    }
}
