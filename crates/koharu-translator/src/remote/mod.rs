mod caiyun;
mod chat;
mod chat_completions;
mod claude;
mod deepl;
mod gemini;
mod google_cloud;

use anyhow::Context;
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;

pub use caiyun::CaiyunConfig;
pub use chat_completions::{
    DeepSeekConfig, OpenAiCompatibleConfig, OpenAiConfig, discover_openai_compatible_models,
};
pub use claude::ClaudeConfig;
pub use deepl::DeepLConfig;
pub use gemini::GeminiConfig;
pub use google_cloud::GoogleCloudConfig;

use crate::{Error, RemoteProviderKind, Result, Translation, TranslationRequest, Translator};

/// An API credential whose `Debug` output is always redacted.
#[derive(Clone)]
pub struct ApiKey(SecretString);

impl ApiKey {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    pub(super) fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiKey([REDACTED])")
    }
}

impl From<String> for ApiKey {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for ApiKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone)]
pub enum RemoteProvider {
    OpenAi(OpenAiConfig),
    Gemini(GeminiConfig),
    Claude(ClaudeConfig),
    DeepSeek(DeepSeekConfig),
    OpenAiCompatible(OpenAiCompatibleConfig),
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
}

#[derive(Debug, Clone)]
pub struct RemoteTranslator {
    client: Client,
    provider: RemoteProvider,
    generation: RemoteGenerationOptions,
}

impl RemoteTranslator {
    #[must_use]
    pub fn new(provider: RemoteProvider) -> Self {
        Self::with_client(Client::new(), provider)
    }

    #[must_use]
    pub fn with_client(client: Client, provider: RemoteProvider) -> Self {
        Self {
            client,
            provider,
            generation: RemoteGenerationOptions::default(),
        }
    }

    #[must_use]
    pub fn with_generation_options(mut self, options: RemoteGenerationOptions) -> Self {
        self.generation = options;
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
                chat_completions::openai(&self.client, config, self.generation, &request).await?
            }
            RemoteProvider::Gemini(config) => {
                gemini::translate(&self.client, config, self.generation, &request).await?
            }
            RemoteProvider::Claude(config) => {
                claude::translate(&self.client, config, self.generation, &request).await?
            }
            RemoteProvider::DeepSeek(config) => {
                chat_completions::deepseek(&self.client, config, self.generation, &request).await?
            }
            RemoteProvider::OpenAiCompatible(config) => {
                chat_completions::compatible(&self.client, config, self.generation, &request)
                    .await?
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
    fn api_keys_are_redacted() {
        assert_eq!(
            format!("{:?}", ApiKey::new("top-secret")),
            "ApiKey([REDACTED])"
        );
    }
}
