use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Context;
use reqwest_middleware::ClientWithMiddleware;

use crate::prompt::{BLOCK_TAG_INSTRUCTIONS, system_prompt};
use crate::{Language, language::tags as language_tags, supported_locales};

/// Resolve the effective system prompt: custom (with block instructions appended) or default.
pub(crate) fn resolve_system_prompt(custom: Option<&str>, target_language: Language) -> String {
    match custom {
        Some(p) if !p.trim().is_empty() => format!("{p} {BLOCK_TAG_INSTRUCTIONS}"),
        _ => system_prompt(target_language),
    }
}

pub mod caiyun;
mod chat_completions;
pub mod claude;
pub mod deepl;
pub mod deepseek;
pub mod gemini;
pub mod google_translate;
pub mod openai;
pub mod openai_compatible;

#[derive(Debug, Clone, Copy)]
pub struct ProviderModelDescriptor {
    pub id: &'static str,
    pub name: &'static str,
}

#[derive(Debug, Clone)]
pub struct DiscoveredProviderModel {
    pub id: String,
    pub name: String,
}

pub type ProviderDiscoveryFuture =
    Pin<Box<dyn Future<Output = anyhow::Result<Vec<DiscoveredProviderModel>>> + Send>>;

pub enum ProviderCatalogModels {
    Static(&'static [ProviderModelDescriptor]),
    Dynamic(fn(ProviderConfig) -> ProviderDiscoveryFuture),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSupportedLanguages {
    All,
    Limited(&'static [Language]),
}

impl ProviderSupportedLanguages {
    pub fn tags(self) -> Vec<String> {
        match self {
            Self::All => supported_locales(),
            Self::Limited(languages) => language_tags(languages),
        }
    }
}

pub struct ProviderDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub requires_api_key: bool,
    pub requires_base_url: bool,
    pub supported_languages: ProviderSupportedLanguages,
    pub models: ProviderCatalogModels,
    pub build: fn(ProviderConfig) -> anyhow::Result<Box<dyn AnyProvider>>,
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
        custom_system_prompt: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>>;
}

#[derive(Clone)]
pub struct ProviderConfig {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

const OPENAI_MODELS: &[ProviderModelDescriptor] = &[ProviderModelDescriptor {
    id: "gpt-5-mini",
    name: "GPT-5 mini",
}];

const GEMINI_MODELS: &[ProviderModelDescriptor] = &[
    ProviderModelDescriptor {
        id: "gemini-3.1-flash-lite-preview",
        name: "Gemini 3.1 Flash-Lite Preview",
    },
    ProviderModelDescriptor {
        id: "gemini-3.1-flash-preview",
        name: "Gemini 3.1 Flash Preview",
    },
    ProviderModelDescriptor {
        id: "gemma4-31b-it",
        name: "Gemma 4 31B",
    },
    ProviderModelDescriptor {
        id: "gemma4-26b-a4b-it",
        name: "Gemma 4 26B",
    },
];

const CLAUDE_MODELS: &[ProviderModelDescriptor] = &[ProviderModelDescriptor {
    id: "claude-haiku-4-5",
    name: "Claude Haiku 4.5",
}];

const DEEPSEEK_MODELS: &[ProviderModelDescriptor] = &[ProviderModelDescriptor {
    id: "deepseek-chat",
    name: "DeepSeek-V3.2-Chat",
}];

const MT_MODELS: &[ProviderModelDescriptor] = &[ProviderModelDescriptor {
    id: "mt",
    name: "Machine Translation",
}];

const PROVIDERS: &[ProviderDescriptor] = &[
    ProviderDescriptor {
        id: "openai",
        name: "OpenAI",
        requires_api_key: true,
        requires_base_url: false,
        supported_languages: ProviderSupportedLanguages::All,
        models: ProviderCatalogModels::Static(OPENAI_MODELS),
        build: build_openai_provider,
    },
    ProviderDescriptor {
        id: "gemini",
        name: "Gemini",
        requires_api_key: true,
        requires_base_url: false,
        supported_languages: ProviderSupportedLanguages::All,
        models: ProviderCatalogModels::Static(GEMINI_MODELS),
        build: build_gemini_provider,
    },
    ProviderDescriptor {
        id: "claude",
        name: "Claude",
        requires_api_key: true,
        requires_base_url: false,
        supported_languages: ProviderSupportedLanguages::All,
        models: ProviderCatalogModels::Static(CLAUDE_MODELS),
        build: build_claude_provider,
    },
    ProviderDescriptor {
        id: "deepseek",
        name: "DeepSeek",
        requires_api_key: true,
        requires_base_url: false,
        supported_languages: ProviderSupportedLanguages::All,
        models: ProviderCatalogModels::Static(DEEPSEEK_MODELS),
        build: build_deepseek_provider,
    },
    ProviderDescriptor {
        id: "deepl",
        name: "DeepL",
        requires_api_key: true,
        requires_base_url: false,
        supported_languages: ProviderSupportedLanguages::All,
        models: ProviderCatalogModels::Static(MT_MODELS),
        build: build_deepl_mt_provider,
    },
    ProviderDescriptor {
        id: "google-translate",
        name: "Google Cloud Translation",
        requires_api_key: true,
        requires_base_url: false,
        supported_languages: ProviderSupportedLanguages::All,
        models: ProviderCatalogModels::Static(MT_MODELS),
        build: build_google_translate_mt_provider,
    },
    ProviderDescriptor {
        id: "caiyun",
        name: "Caiyun",
        requires_api_key: true,
        requires_base_url: false,
        supported_languages: ProviderSupportedLanguages::Limited(
            caiyun::SUPPORTED_TARGET_LANGUAGES,
        ),
        models: ProviderCatalogModels::Static(MT_MODELS),
        build: build_caiyun_mt_provider,
    },
    ProviderDescriptor {
        id: "openai-compatible",
        name: "OpenAI-compatible",
        requires_api_key: false,
        requires_base_url: true,
        supported_languages: ProviderSupportedLanguages::All,
        models: ProviderCatalogModels::Dynamic(discover_openai_compatible_models),
        build: build_openai_compatible_provider,
    },
];

pub fn all_provider_descriptors() -> &'static [ProviderDescriptor] {
    PROVIDERS
}

pub fn find_provider_descriptor(provider_id: &str) -> Option<&'static ProviderDescriptor> {
    PROVIDERS
        .iter()
        .find(|descriptor| descriptor.id == provider_id)
}

pub fn discover_models(
    provider_id: &str,
    config: ProviderConfig,
) -> anyhow::Result<ProviderDiscoveryFuture> {
    let descriptor = find_provider_descriptor(provider_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown API provider: {provider_id}"))?;
    Ok(match descriptor.models {
        ProviderCatalogModels::Static(models) => {
            let models = models
                .iter()
                .map(|model| DiscoveredProviderModel {
                    id: model.id.to_string(),
                    name: model.name.to_string(),
                })
                .collect::<Vec<_>>();
            Box::pin(async move { Ok(models) })
        }
        ProviderCatalogModels::Dynamic(discover) => discover(config),
    })
}

pub fn build_provider(
    provider_id: &str,
    config: ProviderConfig,
) -> anyhow::Result<Box<dyn AnyProvider>> {
    let descriptor = find_provider_descriptor(provider_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown API provider: {provider_id}"))?;

    if descriptor.requires_api_key
        && config
            .api_key
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        anyhow::bail!("api_key is required for {}", descriptor.id);
    }

    if descriptor.requires_base_url
        && config
            .base_url
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        anyhow::bail!("base_url is required for {}", descriptor.id);
    }

    (descriptor.build)(config)
}

fn required_api_key(config: &ProviderConfig, provider_id: &str) -> anyhow::Result<String> {
    config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("api_key is required for {provider_id}"))
}

fn required_base_url(config: &ProviderConfig, provider_id: &str) -> anyhow::Result<String> {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("base_url is required for {provider_id}"))
}

fn build_openai_provider(config: ProviderConfig) -> anyhow::Result<Box<dyn AnyProvider>> {
    Ok(Box::new(openai::OpenAiProvider {
        http_client: Arc::clone(&config.http_client),
        api_key: required_api_key(&config, "openai")?,
    }))
}

fn build_gemini_provider(config: ProviderConfig) -> anyhow::Result<Box<dyn AnyProvider>> {
    Ok(Box::new(gemini::GeminiProvider {
        http_client: Arc::clone(&config.http_client),
        api_key: required_api_key(&config, "gemini")?,
    }))
}

fn build_claude_provider(config: ProviderConfig) -> anyhow::Result<Box<dyn AnyProvider>> {
    Ok(Box::new(claude::ClaudeProvider {
        http_client: Arc::clone(&config.http_client),
        api_key: required_api_key(&config, "claude")?,
    }))
}

fn build_deepseek_provider(config: ProviderConfig) -> anyhow::Result<Box<dyn AnyProvider>> {
    Ok(Box::new(deepseek::DeepSeekProvider {
        http_client: Arc::clone(&config.http_client),
        api_key: required_api_key(&config, "deepseek")?,
    }))
}

fn build_openai_compatible_provider(
    config: ProviderConfig,
) -> anyhow::Result<Box<dyn AnyProvider>> {
    Ok(Box::new(openai_compatible::OpenAiCompatibleProvider {
        http_client: Arc::clone(&config.http_client),
        base_url: required_base_url(&config, "openai-compatible")?,
        api_key: config.api_key,
        temperature: config.temperature,
        max_tokens: config.max_tokens,
    }))
}

fn build_deepl_mt_provider(config: ProviderConfig) -> anyhow::Result<Box<dyn AnyProvider>> {
    Ok(Box::new(deepl::DeeplMtProvider {
        http_client: Arc::clone(&config.http_client),
        api_key: required_api_key(&config, "deepl")?,
        base_url: config.base_url,
    }))
}

fn build_google_translate_mt_provider(
    config: ProviderConfig,
) -> anyhow::Result<Box<dyn AnyProvider>> {
    Ok(Box::new(google_translate::GoogleTranslateMtProvider {
        http_client: Arc::clone(&config.http_client),
        api_key: required_api_key(&config, "google-translate")?,
    }))
}

fn build_caiyun_mt_provider(config: ProviderConfig) -> anyhow::Result<Box<dyn AnyProvider>> {
    Ok(Box::new(caiyun::CaiyunMtProvider {
        http_client: Arc::clone(&config.http_client),
        api_key: required_api_key(&config, "caiyun")?,
    }))
}

fn discover_openai_compatible_models(config: ProviderConfig) -> ProviderDiscoveryFuture {
    Box::pin(async move {
        let base_url = required_base_url(&config, "openai-compatible")?;
        let models = openai_compatible::list_models(
            config.http_client,
            &base_url,
            config.api_key.as_deref(),
        )
        .await?;
        Ok(models
            .into_iter()
            .map(|id| DiscoveredProviderModel {
                name: id.clone(),
                id,
            })
            .collect())
    })
}
