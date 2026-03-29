use std::path::{Path, PathBuf};

use camino::Utf8PathBuf;
use directories::BaseDirs;
use koharu_core::{Config, MirrorKind, MirrorSelection};
use koharu_runtime::{http::DownloadSettings, registry::BootstrapPaths};
use thiserror::Error;
use url::Url;

#[derive(Debug, Clone)]
pub(crate) struct ProjectPaths {
    pub(crate) app_root: Utf8PathBuf,
    pub(crate) config_path: Utf8PathBuf,
    pub(crate) runtime_root: Utf8PathBuf,
    pub(crate) models_root: Utf8PathBuf,
}

impl ProjectPaths {
    pub(crate) fn discover() -> Result<Self, ConfigError> {
        let base_dirs = BaseDirs::new().ok_or(ConfigError::MissingBaseDirs)?;
        let app_root = utf8_path(base_dirs.data_local_dir().join("koharu"))?;
        Ok(Self {
            config_path: app_root.join("config.json"),
            runtime_root: app_root.join("runtime"),
            models_root: app_root.join("models"),
            app_root,
        })
    }
}

#[derive(Debug, Error)]
pub(crate) enum ConfigError {
    #[error("could not resolve local application data directory")]
    MissingBaseDirs,
    #[error("path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
    #[error("failed to read config `{path}`: {source}")]
    ReadConfig {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config `{path}`: {source}")]
    ParseConfig {
        path: Utf8PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to encode config: {0}")]
    EncodeConfig(#[source] serde_json::Error),
    #[error("failed to create directory `{path}`: {source}")]
    CreateDir {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to persist config `{path}`: {source}")]
    PersistConfig {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("runtimePath is required")]
    MissingRuntimePath,
    #[error("modelsPath is required")]
    MissingModelsPath,
    #[error("invalid proxy URL `{value}`: {source}")]
    InvalidProxyUrl {
        value: String,
        #[source]
        source: url::ParseError,
    },
    #[error("invalid {field} mirror URL `{value}`: {source}")]
    InvalidMirrorUrl {
        field: &'static str,
        value: String,
        #[source]
        source: url::ParseError,
    },
    #[error("{field} mirror requires a custom base URL")]
    MissingMirrorUrl { field: &'static str },
    #[error("runtimePath can only be changed during onboarding")]
    RuntimePathLocked,
    #[error("modelsPath can only be changed during onboarding")]
    ModelsPathLocked,
    #[error(transparent)]
    Runtime(#[from] anyhow::Error),
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigStore {
    paths: ProjectPaths,
}

impl ConfigStore {
    pub(crate) fn new(paths: ProjectPaths) -> Self {
        Self { paths }
    }

    pub(crate) fn load(&self) -> Result<Config, ConfigError> {
        let path = &self.paths.config_path;
        if !path.as_std_path().exists() {
            return Ok(default_config(&self.paths));
        }

        let bytes = std::fs::read(path).map_err(|source| ConfigError::ReadConfig {
            path: path.clone(),
            source,
        })?;

        serde_json::from_slice(&bytes).map_err(|source| ConfigError::ParseConfig {
            path: path.clone(),
            source,
        })
    }

    pub(crate) fn save(&self, config: &Config) -> Result<(), ConfigError> {
        ensure_dir(self.paths.app_root.as_std_path(), &self.paths.app_root)?;
        let bytes = serde_json::to_vec_pretty(config).map_err(ConfigError::EncodeConfig)?;
        std::fs::write(&self.paths.config_path, bytes).map_err(|source| ConfigError::PersistConfig {
            path: self.paths.config_path.clone(),
            source,
        })
    }

    pub(crate) fn apply(&self, config: &Config) -> Result<ValidatedConfig, ConfigError> {
        let validated = ValidatedConfig::new(config)?;
        koharu_runtime::http::set_download_settings(validated.download_settings())?;
        Ok(validated)
    }

    pub(crate) fn ensure_paths_locked(
        &self,
        previous: &Config,
        next: &Config,
    ) -> Result<(), ConfigError> {
        if previous.runtime_path != next.runtime_path {
            return Err(ConfigError::RuntimePathLocked);
        }
        if previous.models_path != next.models_path {
            return Err(ConfigError::ModelsPathLocked);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ValidatedConfig {
    runtime_root: Utf8PathBuf,
    models_root: Utf8PathBuf,
    proxy_url: Option<Url>,
    pypi_mirror: ResolvedMirror,
    github_mirror: ResolvedMirror,
}

impl ValidatedConfig {
    fn new(config: &Config) -> Result<Self, ConfigError> {
        Ok(Self {
            runtime_root: parse_required_path(
                &config.runtime_path,
                ConfigError::MissingRuntimePath,
            )?,
            models_root: parse_required_path(
                &config.models_path,
                ConfigError::MissingModelsPath,
            )?,
            proxy_url: parse_optional_url(config.proxy_url.as_deref(), "proxy")?,
            pypi_mirror: ResolvedMirror::new("pypi", &config.pypi_mirror)?,
            github_mirror: ResolvedMirror::new("github", &config.github_mirror)?,
        })
    }

    pub(crate) fn bootstrap_paths(&self) -> BootstrapPaths {
        BootstrapPaths::new(
            self.runtime_root.as_std_path(),
            self.models_root.as_std_path(),
        )
    }

    fn download_settings(&self) -> DownloadSettings {
        DownloadSettings {
            proxy_url: self.proxy_url.as_ref().map(Url::to_string),
            pypi_mirror: self.pypi_mirror.to_wire(),
            github_mirror: self.github_mirror.to_wire(),
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedMirror {
    kind: MirrorKind,
    custom_base_url: Option<Url>,
}

impl ResolvedMirror {
    fn new(field: &'static str, mirror: &MirrorSelection) -> Result<Self, ConfigError> {
        let custom_base_url = parse_optional_url(mirror.custom_base_url.as_deref(), field)?;
        if matches!(mirror.kind, MirrorKind::Custom) && custom_base_url.is_none() {
            return Err(ConfigError::MissingMirrorUrl { field });
        }

        Ok(Self {
            kind: mirror.kind,
            custom_base_url,
        })
    }

    fn to_wire(&self) -> MirrorSelection {
        MirrorSelection {
            kind: self.kind,
            custom_base_url: self.custom_base_url.as_ref().map(Url::to_string),
        }
    }
}

fn default_config(paths: &ProjectPaths) -> Config {
    Config {
        language: "en-US".to_string(),
        runtime_path: paths.runtime_root.to_string(),
        models_path: paths.models_root.to_string(),
        proxy_url: None,
        pypi_mirror: MirrorSelection {
            kind: MirrorKind::Official,
            custom_base_url: None,
        },
        github_mirror: MirrorSelection {
            kind: MirrorKind::Official,
            custom_base_url: None,
        },
    }
}

fn ensure_dir(path: &Path, display: &Utf8PathBuf) -> Result<(), ConfigError> {
    std::fs::create_dir_all(path).map_err(|source| ConfigError::CreateDir {
        path: display.clone(),
        source,
    })
}

fn utf8_path(path: PathBuf) -> Result<Utf8PathBuf, ConfigError> {
    Utf8PathBuf::from_path_buf(path.clone()).map_err(|_| ConfigError::NonUtf8Path(path))
}

fn parse_required_path(value: &str, missing: ConfigError) -> Result<Utf8PathBuf, ConfigError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(missing);
    }
    Ok(Utf8PathBuf::from(value))
}

fn parse_optional_url(
    value: Option<&str>,
    field: &'static str,
) -> Result<Option<Url>, ConfigError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    Url::parse(value).map(Some).map_err(|source| match field {
        "proxy" => ConfigError::InvalidProxyUrl {
            value: value.to_string(),
            source,
        },
        _ => ConfigError::InvalidMirrorUrl {
            field,
            value: value.to_string(),
            source,
        },
    })
}

#[cfg(test)]
mod tests {
    use koharu_core::{MirrorKind, MirrorSelection};

    use super::{ConfigError, ProjectPaths, ValidatedConfig, default_config};

    fn sample_paths() -> ProjectPaths {
        ProjectPaths {
            app_root: "C:/koharu".into(),
            config_path: "C:/koharu/config.json".into(),
            runtime_root: "C:/koharu/runtime".into(),
            models_root: "C:/koharu/models".into(),
        }
    }

    #[test]
    fn rejects_blank_paths() {
        let mut config = default_config(&sample_paths());
        config.runtime_path = "   ".to_string();
        assert!(matches!(
            ValidatedConfig::new(&config),
            Err(ConfigError::MissingRuntimePath)
        ));

        let mut config = default_config(&sample_paths());
        config.models_path = "".to_string();
        assert!(matches!(
            ValidatedConfig::new(&config),
            Err(ConfigError::MissingModelsPath)
        ));
    }

    #[test]
    fn rejects_custom_mirror_without_url() {
        let mut config = default_config(&sample_paths());
        config.pypi_mirror = MirrorSelection {
            kind: MirrorKind::Custom,
            custom_base_url: Some("   ".to_string()),
        };

        assert!(matches!(
            ValidatedConfig::new(&config),
            Err(ConfigError::MissingMirrorUrl { field: "pypi" })
        ));
    }
}
