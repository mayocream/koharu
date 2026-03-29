use anyhow::Context;
use async_trait::async_trait;
use keyring::Entry;
use secrecy::SecretString;
use url::Url;

use self::{
    claude::ClaudeProvider, deepseek::DeepSeekProvider, gemini::GeminiProvider,
    openai::OpenAiProvider, openai_compatible::OpenAiCompatibleProvider,
};
use crate::Language;

pub mod claude;
pub mod deepseek;
pub mod gemini;
pub mod openai;
pub mod openai_compatible;

const API_KEY_SERVICE: &str = "koharu";

fn provider_key_entry(provider: &str) -> anyhow::Result<Entry> {
    let username = format!("llm_provider_api_key_{provider}");
    Ok(Entry::new(API_KEY_SERVICE, &username)?)
}

pub fn get_saved_api_key(provider: &str) -> anyhow::Result<Option<String>> {
    let entry = provider_key_entry(provider)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn set_saved_api_key(provider: &str, api_key: &str) -> anyhow::Result<()> {
    let entry = provider_key_entry(provider)?;
    if api_key.trim().is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.into()),
        }
    } else {
        entry.set_password(api_key)?;
        Ok(())
    }
}

pub async fn ensure_provider_success(
    provider: &str,
    response: reqwest::Response,
) -> anyhow::Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response
        .text()
        .await
        .with_context(|| format!("Failed to read {provider} error response body"))?;
    let body_lower = body.to_ascii_lowercase();
    let quota_exceeded = status.as_u16() == 429
        || body_lower.contains("insufficient_quota")
        || body_lower.contains("quota")
        || body_lower.contains("resource_exhausted")
        || body_lower.contains("rate limit exceeded")
        || body_lower.contains("credit balance is too low");

    if quota_exceeded {
        anyhow::bail!("provider_quota_exceeded:{provider}");
    }

    anyhow::bail!("{provider} API request failed ({status}): {body}");
}

#[async_trait]
pub trait AnyProvider: Send + Sync {
    async fn translate(
        &self,
        source: &str,
        target_language: Language,
        model: &str,
    ) -> anyhow::Result<String>;
}

pub enum Provider {
    OpenAiProvider(OpenAiProvider),
    GeminiProvider(GeminiProvider),
    ClaudeProvider(ClaudeProvider),
    DeepSeekProvider(DeepSeekProvider),
    OpenAiCompatibleProvider(OpenAiCompatibleProvider),
}

#[async_trait]
impl AnyProvider for Provider {
    async fn translate(
        &self,
        source: &str,
        target_language: Language,
        model: &str,
    ) -> anyhow::Result<String> {
        match self {
            Self::OpenAiProvider(provider) => {
                provider.translate(source, target_language, model).await
            }
            Self::GeminiProvider(provider) => {
                provider.translate(source, target_language, model).await
            }
            Self::ClaudeProvider(provider) => {
                provider.translate(source, target_language, model).await
            }
            Self::DeepSeekProvider(provider) => {
                provider.translate(source, target_language, model).await
            }
            Self::OpenAiCompatibleProvider(provider) => {
                provider.translate(source, target_language, model).await
            }
        }
    }
}

pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub custom_system_prompt: Option<String>,
}

pub fn build_provider(provider_id: &str, config: ProviderConfig) -> anyhow::Result<Provider> {
    let required_api_key = |name: &str| {
        config
            .api_key
            .clone()
            .filter(|value| !value.trim().is_empty())
            .map(SecretString::from)
            .ok_or_else(|| anyhow::anyhow!("api_key is required for {name}"))
    };

    let provider = match provider_id {
        "openai" => Provider::OpenAiProvider(openai::OpenAiProvider {
            api_key: required_api_key("openai")?,
        }),
        "gemini" => Provider::GeminiProvider(gemini::GeminiProvider {
            api_key: required_api_key("gemini")?,
        }),
        "claude" => Provider::ClaudeProvider(claude::ClaudeProvider {
            api_key: required_api_key("claude")?,
        }),
        "deepseek" => Provider::DeepSeekProvider(deepseek::DeepSeekProvider {
            api_key: required_api_key("deepseek")?,
        }),
        "openai-compatible" => {
            Provider::OpenAiCompatibleProvider(openai_compatible::OpenAiCompatibleProvider {
                base_url: config
                    .base_url
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("base_url is required for the openai-compatible provider")
                    })?
                    .parse::<Url>()?,
                api_key: config.api_key.map(SecretString::from),
                temperature: config.temperature,
                max_tokens: config.max_tokens,
                custom_system_prompt: config.custom_system_prompt,
            })
        }
        other => anyhow::bail!("Unknown API provider: {other}"),
    };

    Ok(provider)
}
