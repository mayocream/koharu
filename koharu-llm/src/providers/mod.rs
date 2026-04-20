use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use keyring::Entry;
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

const API_KEY_SERVICE: &str = "koharu";
pub const OPENAI_COMPATIBLE_ID: &str = "openai-compatible";

static NO_KEYRING: AtomicBool = AtomicBool::new(false);

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

// ---------------------------------------------------------------------------
// .env file-based fallback for API key storage
// ---------------------------------------------------------------------------

/// Path to the `.env` secrets file inside the app data directory.
fn secrets_env_path() -> std::path::PathBuf {
    koharu_runtime::default_app_data_root()
        .as_std_path()
        .join(".env")
}

/// Read a single provider key from the `.env` file.
/// Lines are expected in `KOHARU_<PROVIDER>_API_KEY=<value>` format.
fn get_file_api_key(provider: &str) -> Option<String> {
    let var_name = env_key_var(provider);
    let content = std::fs::read_to_string(secrets_env_path()).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix(&var_name).and_then(|rest| rest.strip_prefix('='))
        {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// Write (or update) a single provider key in the `.env` file.
/// Creates the file if it doesn't exist. Sets file permissions to 600 on Unix.
fn set_file_api_key(provider: &str, api_key: &str) -> anyhow::Result<()> {
    let path = secrets_env_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory for {}", path.display()))?;
    }

    let var_name = env_key_var(provider);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let mut lines: Vec<String> = Vec::new();
    let mut found = false;
    for line in existing.lines() {
        if line.trim().starts_with(&var_name) && line.contains('=') {
            found = true;
            if !api_key.trim().is_empty() {
                lines.push(format!("{var_name}={api_key}"));
            }
            // If api_key is empty, we skip this line (effectively deleting it)
        } else {
            lines.push(line.to_string());
        }
    }
    if !found && !api_key.trim().is_empty() {
        lines.push(format!("{var_name}={api_key}"));
    }

    let content = if lines.is_empty() {
        String::new()
    } else {
        let mut s = lines.join("\n");
        s.push('\n');
        s
    };
    std::fs::write(&path, &content)
        .with_context(|| format!("failed to write secrets to {}", path.display()))?;

    // Restrict file permissions on Unix (owner read/write only).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms).ok();
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API key accessors (keyring → .env fallback)
// ---------------------------------------------------------------------------

pub fn get_saved_api_key(provider: &str) -> anyhow::Result<Option<String>> {
    if NO_KEYRING.load(Ordering::Relaxed) {
        // --no-keyring: try process env var first, then .env file
        let var = env_key_var(provider);
        let from_env = std::env::var(&var)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        if from_env.is_some() {
            return Ok(from_env);
        }
        return Ok(get_file_api_key(provider));
    }

    // Normal mode: try keyring first
    match provider_key_entry(provider).and_then(|entry| {
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }) {
        Ok(value) => Ok(value),
        Err(err) => {
            tracing::debug!(provider, "keyring read failed, trying .env fallback: {err}");
            Ok(get_file_api_key(provider))
        }
    }
}

pub fn set_saved_api_key(provider: &str, api_key: &str) -> anyhow::Result<()> {
    if NO_KEYRING.load(Ordering::Relaxed) {
        // --no-keyring: write to .env file directly
        let path = secrets_env_path();
        tracing::info!(provider, path = %path.display(), "keyring disabled, saving API key to .env");
        return set_file_api_key(provider, api_key);
    }

    // Normal mode: try keyring, fall back to .env on failure
    let keyring_result = provider_key_entry(provider).and_then(|entry| {
        if api_key.trim().is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(err) => Err(err.into()),
            }
        } else {
            entry.set_password(api_key)?;
            Ok(())
        }
    });

    match keyring_result {
        Ok(()) => Ok(()),
        Err(err) => {
            tracing::warn!(
                provider,
                "keyring write failed, saving to .env fallback: {err}"
            );
            set_file_api_key(provider, api_key)
        }
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

const GEMINI_MODELS: &[ProviderModelDescriptor] = &[ProviderModelDescriptor {
    id: "gemini-3.1-flash-lite-preview",
    name: "Gemini 3.1 Flash-Lite Preview",
}];

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
        id: OPENAI_COMPATIBLE_ID,
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
        base_url: required_base_url(&config, OPENAI_COMPATIBLE_ID)?,
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
        let base_url = required_base_url(&config, OPENAI_COMPATIBLE_ID)?;
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
