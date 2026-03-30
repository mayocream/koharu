use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use koharu_core::{BootstrapConfig, BootstrapHttpConfig, BootstrapPathConfig};
use koharu_runtime::Settings;
use tempfile::NamedTempFile;
use url::Url;

const CONFIG_DIR: &str = ".koharu";
const CONFIG_FILE: &str = "config.toml";

pub type AppConfig = Settings;

pub fn config_dir() -> Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("failed to resolve home directory"))?;
    Ok(home.join(CONFIG_DIR))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(CONFIG_FILE))
}

pub fn load() -> Result<AppConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read `{}`", path.display()))?;
    toml::from_str(&content).with_context(|| format!("failed to parse `{}`", path.display()))
}

pub fn save(config: &AppConfig) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create config dir `{}`", dir.display()))?;

    let path = dir.join(CONFIG_FILE);
    let content = toml::to_string_pretty(config).context("failed to serialize app config")?;
    let mut temp = NamedTempFile::new_in(&dir)
        .with_context(|| format!("failed to stage `{}`", path.display()))?;
    temp.write_all(content.as_bytes())
        .with_context(|| format!("failed to write temp config for `{}`", path.display()))?;
    temp.flush()
        .with_context(|| format!("failed to flush temp config for `{}`", path.display()))?;

    match temp.persist(&path) {
        Ok(_) => Ok(()),
        Err(err) => {
            if path.exists() {
                fs::remove_file(&path).with_context(|| {
                    format!("failed to replace existing config `{}`", path.display())
                })?;
            }
            err.file.persist(&path).map(|_| ()).map_err(|persist_err| {
                anyhow::anyhow!(
                    "failed to persist config to `{}`: {}",
                    path.display(),
                    persist_err.error
                )
            })
        }
    }
}

pub fn to_bootstrap_config(config: &AppConfig) -> BootstrapConfig {
    BootstrapConfig {
        runtime: BootstrapPathConfig {
            path: config.runtime.path.to_string_lossy().to_string(),
        },
        models: BootstrapPathConfig {
            path: config.models.path.to_string_lossy().to_string(),
        },
        http: BootstrapHttpConfig {
            proxy: config.http_proxy().map(Url::to_string),
        },
    }
}

pub fn from_bootstrap_config(config: BootstrapConfig) -> Result<AppConfig> {
    let runtime_path = config.runtime.path.trim();
    let models_path = config.models.path.trim();
    anyhow::ensure!(!runtime_path.is_empty(), "runtime path is required");
    anyhow::ensure!(!models_path.is_empty(), "models path is required");

    let proxy = config
        .http
        .proxy
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(Url::parse)
        .transpose()
        .context("invalid HTTP proxy URL")?;

    Ok(
        AppConfig::from_paths(PathBuf::from(runtime_path), PathBuf::from(models_path))
            .with_proxy(proxy),
    )
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::Path;

    use super::{
        AppConfig, BootstrapConfig, BootstrapHttpConfig, BootstrapPathConfig,
        from_bootstrap_config, to_bootstrap_config,
    };

    #[test]
    fn bootstrap_round_trip_preserves_paths_and_proxy() {
        let config = BootstrapConfig {
            runtime: BootstrapPathConfig {
                path: "/tmp/runtime".to_string(),
            },
            models: BootstrapPathConfig {
                path: "/tmp/models".to_string(),
            },
            http: BootstrapHttpConfig {
                proxy: Some("http://127.0.0.1:7890".to_string()),
            },
        };

        let app = from_bootstrap_config(config.clone()).unwrap();
        let serialized = to_bootstrap_config(&app);

        assert_eq!(serialized.runtime.path, config.runtime.path);
        assert_eq!(serialized.models.path, config.models.path);
        assert_eq!(
            serialized
                .http
                .proxy
                .as_deref()
                .map(url::Url::parse)
                .transpose()
                .unwrap(),
            config
                .http
                .proxy
                .as_deref()
                .map(url::Url::parse)
                .transpose()
                .unwrap()
        );
    }

    #[test]
    fn blank_proxy_becomes_none() {
        let app = from_bootstrap_config(BootstrapConfig {
            runtime: BootstrapPathConfig {
                path: ".".to_string(),
            },
            models: BootstrapPathConfig {
                path: ".".to_string(),
            },
            http: BootstrapHttpConfig {
                proxy: Some("   ".to_string()),
            },
        })
        .unwrap();

        assert!(app.http_proxy().is_none());
    }

    #[test]
    fn empty_paths_are_rejected() {
        let error = from_bootstrap_config(BootstrapConfig {
            runtime: BootstrapPathConfig {
                path: "   ".to_string(),
            },
            models: BootstrapPathConfig {
                path: ".".to_string(),
            },
            http: BootstrapHttpConfig::default(),
        })
        .expect_err("empty runtime path must be rejected");

        assert!(error.to_string().contains("runtime path is required"));
    }

    #[test]
    fn defaults_use_runtime_owned_paths() {
        let config = AppConfig::default();
        assert!(
            config.runtime.path.is_absolute() || config.runtime.path == Path::new("").to_path_buf()
        );
        assert!(
            config.models.path.is_absolute() || config.models.path == Path::new("").to_path_buf()
        );
    }
    #[test]
    fn config_path_uses_home_dir_layout() {
        let path = super::config_path().unwrap();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("config.toml")
        );
        assert!(path.to_string_lossy().contains(".koharu"));
        let _ = env::var_os("HOME");
    }
}
