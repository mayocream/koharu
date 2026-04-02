use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use koharu_core::{
    AppConfig as PublicAppConfig, AppConfigUpdate, AppDataConfig, AppLlmConfig,
    AppLlmProviderConfig, AppLlmProviderConfigUpdate,
};
use koharu_llm::providers::{
    all_provider_descriptors, find_provider_descriptor, get_saved_api_key, set_saved_api_key,
};
use koharu_runtime::default_app_data_root;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

const CONFIG_FILE: &str = "config.toml";
const MANAGED_DATA_DIRS: &[&str] = &["runtime", "models", "blobs", "pages"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StoredConfig {
    pub data: StoredDataConfig,
    pub llm: StoredLlmConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StoredDataConfig {
    pub path: Utf8PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StoredLlmConfig {
    pub providers: BTreeMap<String, StoredProviderConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StoredProviderConfig {
    pub base_url: Option<String>,
}

impl Default for StoredDataConfig {
    fn default() -> Self {
        Self {
            path: default_app_data_root(),
        }
    }
}

pub fn config_path() -> Result<Utf8PathBuf> {
    Ok(default_app_data_root().join(CONFIG_FILE))
}

pub fn load() -> Result<StoredConfig> {
    let path = config_path()?;
    if !path.exists() {
        let config = StoredConfig::default();
        save(&config)?;
        return Ok(config);
    }

    let content = fs::read_to_string(&path).with_context(|| format!("failed to read `{path}`"))?;
    toml::from_str(&content).with_context(|| format!("failed to parse `{path}`"))
}

pub fn save(config: &StoredConfig) -> Result<()> {
    let path = config_path()?;
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path `{path}` does not have a parent directory"))?;
    fs::create_dir_all(dir).with_context(|| format!("failed to create config dir `{dir}`"))?;

    let content = toml::to_string_pretty(config).context("failed to serialize app config")?;
    let mut temp =
        NamedTempFile::new_in(dir).with_context(|| format!("failed to stage `{path}`"))?;
    temp.write_all(content.as_bytes())
        .with_context(|| format!("failed to write temp config for `{path}`"))?;
    temp.flush()
        .with_context(|| format!("failed to flush temp config for `{path}`"))?;

    match temp.persist(&path) {
        Ok(_) => Ok(()),
        Err(err) => {
            if path.exists() {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to replace existing config `{path}`"))?;
            }
            err.file.persist(&path).map(|_| ()).map_err(|persist_err| {
                anyhow::anyhow!(
                    "failed to persist config to `{path}`: {}",
                    persist_err.error
                )
            })
        }
    }
}

pub fn to_public_config(config: &StoredConfig) -> Result<PublicAppConfig> {
    let mut providers = Vec::new();
    for descriptor in all_provider_descriptors() {
        let stored = config.llm.providers.get(descriptor.id);
        let has_api_key = get_saved_api_key(descriptor.id)?
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        providers.push(AppLlmProviderConfig {
            id: descriptor.id.to_string(),
            base_url: stored.and_then(|provider| provider.base_url.clone()),
            has_api_key,
        });
    }

    Ok(PublicAppConfig {
        data: AppDataConfig {
            path: config.data.path.to_string(),
        },
        llm: AppLlmConfig { providers },
    })
}

pub fn from_public_update(config: AppConfigUpdate) -> Result<StoredConfig> {
    validate_provider_updates(&config)?;

    let data_path = config.data.path.trim();
    anyhow::ensure!(!data_path.is_empty(), "data path is required");
    let data_path = Utf8PathBuf::from(data_path);
    anyhow::ensure!(data_path.is_absolute(), "data path must be absolute");

    let mut providers = BTreeMap::new();
    for provider in config.llm.providers {
        let id = provider.id.trim();
        let descriptor = find_provider_descriptor(id)
            .ok_or_else(|| anyhow::anyhow!("unknown provider id: {id}"))?;
        let base_url = normalized_base_url(&provider);

        if base_url.is_some()
            || provider.api_key.is_some()
            || provider.clear_api_key
            || descriptor.requires_base_url
        {
            providers.insert(id.to_string(), StoredProviderConfig { base_url });
        }
    }

    Ok(StoredConfig {
        data: StoredDataConfig { path: data_path },
        llm: StoredLlmConfig { providers },
    })
}

pub fn move_app_data_if_needed(current: &StoredConfig, next: &StoredConfig) -> Result<bool> {
    if same_path(&current.data.path, &next.data.path) {
        return Ok(false);
    }

    fs::create_dir_all(&next.data.path)
        .with_context(|| format!("failed to create `{}`", next.data.path))?;

    for name in MANAGED_DATA_DIRS {
        move_path(
            current.data.path.join(name).as_std_path(),
            next.data.path.join(name).as_std_path(),
        )?;
    }

    Ok(true)
}

pub fn apply_secret_updates(config: &AppConfigUpdate) -> Result<()> {
    validate_provider_updates(config)?;

    for provider in &config.llm.providers {
        let id = provider.id.trim();
        let api_key = provider.api_key.as_deref().map(str::trim);
        let should_clear = provider.clear_api_key || matches!(api_key, Some(""));
        if should_clear {
            set_saved_api_key(id, "")?;
            continue;
        }
        if let Some(api_key) = api_key
            && !api_key.is_empty()
        {
            set_saved_api_key(id, api_key)?;
        }
    }

    Ok(())
}

fn same_path(left: &Utf8Path, right: &Utf8Path) -> bool {
    match (
        fs::canonicalize(left.as_std_path()),
        fs::canonicalize(right.as_std_path()),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn move_path(source: &std::path::Path, destination: &std::path::Path) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }

    if !destination.exists() && fs::rename(source, destination).is_ok() {
        return Ok(());
    }

    if source.is_dir() {
        fs::create_dir_all(destination)
            .with_context(|| format!("failed to create `{}`", destination.display()))?;

        for entry in fs::read_dir(source)
            .with_context(|| format!("failed to read `{}`", source.display()))?
        {
            let entry = entry.with_context(|| format!("failed to read `{}`", source.display()))?;
            move_path(&entry.path(), &destination.join(entry.file_name()))?;
        }

        fs::remove_dir_all(source)
            .with_context(|| format!("failed to remove `{}`", source.display()))?;
    } else {
        if destination.exists() {
            fs::remove_file(destination)
                .with_context(|| format!("failed to replace `{}`", destination.display()))?;
        }
        fs::copy(source, destination).with_context(|| {
            format!(
                "failed to copy `{}` to `{}`",
                source.display(),
                destination.display()
            )
        })?;
        fs::remove_file(source)
            .with_context(|| format!("failed to remove `{}`", source.display()))?;
    }

    Ok(())
}
fn normalized_base_url(provider: &AppLlmProviderConfigUpdate) -> Option<String> {
    provider
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn validate_provider_updates(config: &AppConfigUpdate) -> Result<()> {
    let mut seen = HashSet::new();
    for provider in &config.llm.providers {
        let id = provider.id.trim();
        anyhow::ensure!(!id.is_empty(), "provider id is required");
        anyhow::ensure!(
            find_provider_descriptor(id).is_some(),
            "unknown provider id: {id}"
        );
        anyhow::ensure!(seen.insert(id.to_string()), "duplicate provider id: {id}");
        anyhow::ensure!(
            !(provider.clear_api_key
                && provider
                    .api_key
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty())),
            "provider {id} cannot set and clear api_key in the same update"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;
    use koharu_core::{
        AppConfigUpdate, AppDataConfigUpdate, AppLlmConfigUpdate, AppLlmProviderConfigUpdate,
    };

    use super::{
        StoredConfig, config_path, from_public_update, move_app_data_if_needed, to_public_config,
    };

    #[test]
    fn old_config_without_data_section_still_loads() {
        let config: StoredConfig = toml::from_str(
            r#"
                [runtime]
                path = "/tmp/runtime"

                [models]
                path = "/tmp/models"

                [http]
                proxy = "http://127.0.0.1:7890"
            "#,
        )
        .unwrap();

        assert_eq!(config.data.path, koharu_runtime::default_app_data_root());
        assert!(config.llm.providers.is_empty());
    }

    #[test]
    fn public_update_round_trip_preserves_fields() {
        let config = from_public_update(AppConfigUpdate {
            data: AppDataConfigUpdate {
                path: "/tmp/koharu-data".to_string(),
            },
            llm: AppLlmConfigUpdate {
                providers: vec![AppLlmProviderConfigUpdate {
                    id: "openai-compatible".to_string(),
                    base_url: Some(" http://127.0.0.1:1234/v1/ ".to_string()),
                    api_key: None,
                    clear_api_key: false,
                }],
            },
        })
        .unwrap();

        let public = to_public_config(&config).unwrap();

        assert_eq!(public.data.path, "/tmp/koharu-data");
        assert_eq!(
            public
                .llm
                .providers
                .iter()
                .find(|provider| provider.id == "openai-compatible")
                .and_then(|provider| provider.base_url.as_deref()),
            Some("http://127.0.0.1:1234/v1/")
        );
    }

    #[test]
    fn public_update_requires_absolute_data_path() {
        let error = from_public_update(AppConfigUpdate {
            data: AppDataConfigUpdate {
                path: "relative\\koharu-data".to_string(),
            },
            llm: AppLlmConfigUpdate::default(),
        })
        .unwrap_err();

        assert!(error.to_string().contains("absolute"));
    }

    #[test]
    fn move_app_data_moves_managed_directories() {
        let tempdir = tempfile::tempdir().unwrap();
        let current = tempdir.path().join("old");
        let next = tempdir.path().join("new");
        fs::create_dir_all(current.join("pages")).unwrap();
        fs::create_dir_all(current.join("runtime")).unwrap();
        fs::write(current.join("pages").join("doc.json"), b"{}").unwrap();
        fs::write(current.join("runtime").join("pkg.bin"), b"runtime").unwrap();

        let current_config = StoredConfig {
            data: super::StoredDataConfig {
                path: Utf8PathBuf::from_path_buf(current.clone()).unwrap(),
            },
            ..StoredConfig::default()
        };
        let next_config = StoredConfig {
            data: super::StoredDataConfig {
                path: Utf8PathBuf::from_path_buf(next.clone()).unwrap(),
            },
            ..StoredConfig::default()
        };

        let moved = move_app_data_if_needed(&current_config, &next_config).unwrap();

        assert!(moved);
        assert!(next.join("pages").join("doc.json").exists());
        assert!(next.join("runtime").join("pkg.bin").exists());
        assert!(!current.join("pages").exists());
        assert!(!current.join("runtime").exists());
    }

    #[test]
    fn config_path_uses_appdata_layout() {
        let path = config_path().unwrap();
        assert_eq!(path.file_name(), Some("config.toml"));
        assert!(path.as_str().contains("Koharu"));
    }
}
