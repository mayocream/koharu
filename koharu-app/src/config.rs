use std::fs;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use koharu_runtime::{SecretStore, default_app_data_root};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::ToSchema;

use crate::pipeline::{Artifact, Registry};

const CONFIG_FILE: &str = "config.toml";
const REDACTED: &str = "[REDACTED]";
const SECRET_SERVICE: &str = "koharu";
const PROVIDER_API_KEY_SECRET_PREFIX: &str = "llm_provider_api_key_";

// ---------------------------------------------------------------------------
// RedactedSecret
// ---------------------------------------------------------------------------

/// A secret value that serializes as `"[REDACTED]"` but deserializes normally.
#[derive(Clone)]
pub struct RedactedSecret(secrecy::SecretString);

impl RedactedSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(secrecy::SecretString::from(value.into()))
    }

    pub fn expose(&self) -> &str {
        use secrecy::ExposeSecret;
        self.0.expose_secret()
    }
}

impl std::fmt::Debug for RedactedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(REDACTED)
    }
}

impl Serialize for RedactedSecret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

impl<'de> Deserialize<'de> for RedactedSecret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self::new(s))
    }
}

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct AppConfig {
    pub data: DataConfig,
    pub http: HttpConfig,
    pub pipeline: PipelineConfig,
    pub providers: Vec<ProviderConfig>,
}

/// Engine selection for each pipeline stage.
/// Values are engine IDs (e.g. "pp-doclayout-v3", "comic-text-detector").
/// Empty string means use default.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct PipelineConfig {
    pub detector: String,
    pub font_detector: String,
    pub segmenter: String,
    pub bubble_segmenter: String,
    pub ocr: String,
    pub translator: String,
    pub inpainter: String,
    pub renderer: String,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            detector: "pp-doclayout-v3".to_string(),
            font_detector: "yuzumarker-font-detection".to_string(),
            segmenter: "comic-text-detector-seg".to_string(),
            bubble_segmenter: "speech-bubble-segmentation".to_string(),
            ocr: "paddle-ocr-vl-1.5".to_string(),
            translator: "llm".to_string(),
            inpainter: "lama-manga".to_string(),
            renderer: "koharu-renderer".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataConfig {
    #[schema(value_type = String)]
    pub path: Utf8PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct HttpConfig {
    pub connect_timeout: u64,
    pub read_timeout: u64,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProviderConfig {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Populated from credential storage on `load()`, never written to config.toml.
    /// Serializes as `"[REDACTED]"` in API responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub api_key: Option<RedactedSecret>,
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            path: default_app_data_root(),
        }
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            connect_timeout: 20,
            read_timeout: 300,
            max_retries: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Load / save
// ---------------------------------------------------------------------------

pub fn config_path() -> Result<Utf8PathBuf> {
    Ok(default_app_data_root().join(CONFIG_FILE))
}

pub fn load() -> Result<AppConfig> {
    let path = config_path()?;
    let mut config: AppConfig = if path.exists() {
        let content =
            fs::read_to_string(&path).with_context(|| format!("failed to read `{path}`"))?;
        toml::from_str(&content).with_context(|| format!("failed to parse `{path}`"))?
    } else {
        let config = AppConfig::default();
        save(&config)?;
        config
    };

    if validate_pipeline_config(&mut config) {
        save(&config)?;
    }

    // Populate api_key from credential storage for every known provider.
    let secrets = SecretStore::new(SECRET_SERVICE);
    for provider in &mut config.providers {
        if let Ok(Some(key)) = secrets.get(&provider_api_key_secret_key(&provider.id))
            && !key.trim().is_empty()
        {
            provider.api_key = Some(RedactedSecret::new(key));
        }
    }

    Ok(config)
}

pub fn save(config: &AppConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir `{parent}`"))?;
    }
    // `api_key` is `#[serde(skip)]`, so it is never written to the TOML file.
    let content = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(&path, content).with_context(|| format!("failed to write config to `{path}`"))
}

// ---------------------------------------------------------------------------
// Patch application
// ---------------------------------------------------------------------------

/// Apply a `ConfigPatch` in-place. Missing fields leave the existing value
/// alone. Providers are replaced wholesale (the list, not field-by-field).
pub fn apply_patch(config: &mut AppConfig, patch: koharu_core::ConfigPatch) {
    if let Some(data) = patch.data
        && let Some(path) = data.path
    {
        config.data.path = camino::Utf8PathBuf::from(path);
    }
    if let Some(http) = patch.http {
        if let Some(v) = http.connect_timeout {
            config.http.connect_timeout = v;
        }
        if let Some(v) = http.read_timeout {
            config.http.read_timeout = v;
        }
        if let Some(v) = http.max_retries {
            config.http.max_retries = v;
        }
    }
    if let Some(p) = patch.pipeline {
        if let Some(v) = p.detector {
            config.pipeline.detector = v;
        }
        if let Some(v) = p.font_detector {
            config.pipeline.font_detector = v;
        }
        if let Some(v) = p.segmenter {
            config.pipeline.segmenter = v;
        }
        if let Some(v) = p.bubble_segmenter {
            config.pipeline.bubble_segmenter = v;
        }
        if let Some(v) = p.ocr {
            config.pipeline.ocr = v;
        }
        if let Some(v) = p.translator {
            config.pipeline.translator = v;
        }
        if let Some(v) = p.inpainter {
            config.pipeline.inpainter = v;
        }
        if let Some(v) = p.renderer {
            config.pipeline.renderer = v;
        }
    }
    if let Some(providers) = patch.providers {
        let mut new_providers = Vec::with_capacity(providers.len());
        for p in providers {
            let existing = config.providers.iter().find(|e| e.id == p.id);
            let api_key = match p.api_key.as_deref() {
                Some(REDACTED) => existing.and_then(|e| e.api_key.clone()),
                Some("") => None,
                Some(s) => Some(RedactedSecret::new(s)),
                None => existing.and_then(|e| e.api_key.clone()),
            };
            new_providers.push(ProviderConfig {
                id: p.id,
                base_url: p
                    .base_url
                    .or_else(|| existing.and_then(|e| e.base_url.clone())),
                api_key,
            });
        }
        config.providers = new_providers;
    }

    validate_pipeline_config(config);
}

fn validate_pipeline_config(config: &mut AppConfig) -> bool {
    let defaults = PipelineConfig::default();
    let mut changed = false;

    changed |= validate_engine_name(
        "detector",
        &mut config.pipeline.detector,
        &defaults.detector,
        Artifact::TextBoxes,
    );
    changed |= validate_engine_name(
        "font_detector",
        &mut config.pipeline.font_detector,
        &defaults.font_detector,
        Artifact::FontPredictions,
    );
    changed |= validate_engine_name(
        "segmenter",
        &mut config.pipeline.segmenter,
        &defaults.segmenter,
        Artifact::SegmentMask,
    );
    changed |= validate_engine_name(
        "bubble_segmenter",
        &mut config.pipeline.bubble_segmenter,
        &defaults.bubble_segmenter,
        Artifact::BubbleMask,
    );
    changed |= validate_engine_name(
        "ocr",
        &mut config.pipeline.ocr,
        &defaults.ocr,
        Artifact::OcrText,
    );
    changed |= validate_engine_name(
        "translator",
        &mut config.pipeline.translator,
        &defaults.translator,
        Artifact::Translations,
    );
    changed |= validate_engine_name(
        "inpainter",
        &mut config.pipeline.inpainter,
        &defaults.inpainter,
        Artifact::Inpainted,
    );
    changed |= validate_engine_name(
        "renderer",
        &mut config.pipeline.renderer,
        &defaults.renderer,
        Artifact::FinalRender,
    );

    changed
}

fn validate_engine_name(
    field: &'static str,
    configured: &mut String,
    default: &str,
    artifact: Artifact,
) -> bool {
    let trimmed = configured.trim();
    let is_valid = !trimmed.is_empty()
        && Registry::providers(artifact)
            .into_iter()
            .any(|engine| engine.id == trimmed);

    if is_valid {
        if trimmed != configured {
            *configured = trimmed.to_string();
            return true;
        }
        return false;
    }

    if trimmed != default {
        tracing::warn!(
            field,
            configured_engine = configured.as_str(),
            default_engine = default,
            "invalid pipeline engine in config; resetting to default"
        );
    }
    *configured = default.to_string();
    true
}

// ---------------------------------------------------------------------------
// Secret handling
// ---------------------------------------------------------------------------

/// Sync api_key fields to credential storage.
/// - `Some(RedactedSecret)` with value != "[REDACTED]" → save to credential storage
/// - `None` → clear from credential storage
/// - `Some(RedactedSecret)` with value == "[REDACTED]" → unchanged
pub fn sync_secrets(config: &AppConfig) -> Result<()> {
    let secrets = SecretStore::new(SECRET_SERVICE);
    for provider in &config.providers {
        match &provider.api_key {
            Some(secret) if secret.expose() != REDACTED => {
                let key = provider_api_key_secret_key(&provider.id);
                if secret.expose().trim().is_empty() {
                    secrets.delete(&key)?;
                } else {
                    secrets.set(&key, secret.expose())?;
                }
            }
            None => {
                secrets.delete(&provider_api_key_secret_key(&provider.id))?;
            }
            _ => {} // "[REDACTED]" means unchanged
        }
    }
    Ok(())
}

fn provider_api_key_secret_key(provider_id: &str) -> String {
    format!("{PROVIDER_API_KEY_SECRET_PREFIX}{provider_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_core::{ConfigPatch, PipelineConfigPatch};

    #[test]
    fn old_config_without_providers_still_loads() {
        let config: AppConfig = toml::from_str(
            r#"
                [data]
                path = "/tmp/test"
            "#,
        )
        .unwrap();

        assert_eq!(config.data.path, "/tmp/test");
        assert_eq!(config.http.connect_timeout, 20);
        assert_eq!(config.http.read_timeout, 300);
        assert_eq!(config.http.max_retries, 3);
        assert!(config.providers.is_empty());
    }

    #[test]
    fn partial_http_config_uses_defaults_for_missing_fields() {
        let config: AppConfig = toml::from_str(
            r#"
                [http]
                connect_timeout = 45
            "#,
        )
        .unwrap();

        assert_eq!(config.http.connect_timeout, 45);
        assert_eq!(config.http.read_timeout, 300);
        assert_eq!(config.http.max_retries, 3);
    }

    #[test]
    fn config_path_uses_appdata_layout() {
        let path = config_path().unwrap();
        assert_eq!(path.file_name(), Some("config.toml"));
        assert!(path.as_str().contains("Koharu"));
    }

    #[test]
    fn provider_api_key_secret_key_preserves_legacy_keyring_user() {
        assert_eq!(
            provider_api_key_secret_key("openai"),
            "llm_provider_api_key_openai"
        );
    }

    #[test]
    fn invalid_pipeline_engines_reset_to_defaults() {
        let mut config = AppConfig::default();
        config.pipeline.detector = "bad-detector".to_string();
        config.pipeline.renderer = "bad-renderer".to_string();
        config.pipeline.ocr = String::new();

        let changed = validate_pipeline_config(&mut config);

        assert!(changed);
        assert_eq!(config.pipeline.detector, PipelineConfig::default().detector);
        assert_eq!(config.pipeline.renderer, PipelineConfig::default().renderer);
        assert_eq!(config.pipeline.ocr, PipelineConfig::default().ocr);
    }

    #[test]
    fn apply_patch_normalizes_invalid_pipeline_engine_names() {
        let mut config = AppConfig::default();
        apply_patch(
            &mut config,
            ConfigPatch {
                pipeline: Some(PipelineConfigPatch {
                    renderer: Some("not-a-renderer".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        assert_eq!(config.pipeline.renderer, PipelineConfig::default().renderer);
    }
}
