use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use keyring::Entry;
use reqwest_middleware::ClientWithMiddleware;

use crate::Language;

pub mod claude;
pub mod deepseek;
pub mod gemini;
pub mod openai;
pub mod openai_compatible;

const API_KEY_SERVICE: &str = "koharu";

static NO_KEYRING: AtomicBool = AtomicBool::new(false);

pub fn disable_keyring() {
    NO_KEYRING.store(true, Ordering::Relaxed);
}

fn env_key_var(provider: &str) -> String {
    format!(
        "KOHARU_{}_API_KEY",
        provider.to_ascii_uppercase().replace('-', "_")
    )
}

fn provider_key_entry(provider: &str) -> anyhow::Result<Entry> {
    let username = format!("llm_provider_api_key_{provider}");
    Ok(Entry::new(API_KEY_SERVICE, &username)?)
}

pub fn get_saved_api_key(provider: &str) -> anyhow::Result<Option<String>> {
    if NO_KEYRING.load(Ordering::Relaxed) {
        let var = env_key_var(provider);
        return Ok(
            std::env::var(&var)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
        );
    }

    let entry = provider_key_entry(provider)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn set_saved_api_key(provider: &str, api_key: &str) -> anyhow::Result<()> {
    if NO_KEYRING.load(Ordering::Relaxed) {
        tracing::warn!(
            provider,
            "keyring is disabled; API key changes are not saved"
        );
        return Err(anyhow::anyhow!("keyring is disabled; API key cannot be saved"));
    }

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

pub trait AnyProvider: Send + Sync {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        model: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>>;
}

pub struct ProviderConfig {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub custom_system_prompt: Option<String>,
}

pub fn build_provider(
    provider_id: &str,
    config: ProviderConfig,
) -> anyhow::Result<Box<dyn AnyProvider>> {
    let required_api_key = |name: &str| {
        config
            .api_key
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("api_key is required for {name}"))
    };

    let provider: Box<dyn AnyProvider> = match provider_id {
        "openai" => Box::new(openai::OpenAiProvider {
            http_client: Arc::clone(&config.http_client),
            api_key: required_api_key("openai")?,
        }),
        "gemini" => Box::new(gemini::GeminiProvider {
            http_client: Arc::clone(&config.http_client),
            api_key: required_api_key("gemini")?,
        }),
        "claude" => Box::new(claude::ClaudeProvider {
            http_client: Arc::clone(&config.http_client),
            api_key: required_api_key("claude")?,
        }),
        "deepseek" => Box::new(deepseek::DeepSeekProvider {
            http_client: Arc::clone(&config.http_client),
            api_key: required_api_key("deepseek")?,
        }),
        "openai-compatible" => Box::new(openai_compatible::OpenAiCompatibleProvider {
            http_client: Arc::clone(&config.http_client),
            base_url: config
                .base_url
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("base_url is required for the openai-compatible provider")
                })?,
            api_key: config.api_key,
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            custom_system_prompt: config.custom_system_prompt,
        }),
        other => anyhow::bail!("Unknown API provider: {other}"),
    };

    Ok(provider)
}
