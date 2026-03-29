use std::io::Cursor;
use std::path::{Path, PathBuf};

use atomicwrites::{AllowOverwrite, AtomicFile};
use camino::Utf8PathBuf;
use directories::BaseDirs;
use koharu_core::{Config, MirrorKind, MirrorSelection};
use koharu_runtime::http::DownloadSettings;
use koharu_runtime::registry::BootstrapPaths;
use serde_path_to_error::Segment;
use thiserror::Error;
use typed_builder::TypedBuilder;
use url::Url;

#[derive(Debug, Clone, TypedBuilder)]
pub(crate) struct ProjectPaths {
    pub(crate) app_root: Utf8PathBuf,
    pub(crate) config_path: Utf8PathBuf,
    pub(crate) default_runtime_root: Utf8PathBuf,
    pub(crate) default_models_root: Utf8PathBuf,
}

impl ProjectPaths {
    pub(crate) fn discover() -> Result<Self, BootstrapConfigError> {
        let base_dirs = BaseDirs::new().ok_or(BootstrapConfigError::MissingBaseDirs)?;
        let app_root = utf8_path(base_dirs.data_local_dir().join("koharu"))?;
        Ok(Self::builder()
            .config_path(app_root.join("config.json"))
            .default_runtime_root(app_root.join("runtime"))
            .default_models_root(app_root.join("models"))
            .app_root(app_root)
            .build())
    }
}

#[derive(Debug, Error)]
pub(crate) enum BootstrapConfigError {
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
    #[error("failed to parse config at `{path}`: {source}")]
    ParseConfig {
        path: String,
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

#[derive(Debug, Clone, TypedBuilder)]
pub(crate) struct ConfigStore {
    paths: ProjectPaths,
}

impl ConfigStore {
    pub(crate) fn new(paths: ProjectPaths) -> Self {
        Self { paths }
    }

    pub(crate) fn load(&self) -> Result<Config, BootstrapConfigError> {
        let path = &self.paths.config_path;
        if path.as_std_path().exists() {
            let bytes = std::fs::read(path).map_err(|source| BootstrapConfigError::ReadConfig {
                path: path.clone(),
                source,
            })?;
            return parse_config(&bytes);
        }

        Ok(default_config(&self.paths))
    }

    pub(crate) fn persist(&self, config: &Config) -> Result<(), BootstrapConfigError> {
        ensure_dir(self.paths.app_root.as_std_path(), &self.paths.app_root)?;
        let bytes =
            serde_json::to_vec_pretty(config).map_err(BootstrapConfigError::EncodeConfig)?;
        let file = AtomicFile::new(&self.paths.config_path, AllowOverwrite);
        file.write(|target| {
            use std::io::Write;
            target.write_all(&bytes)
        })
        .map_err(|source| BootstrapConfigError::PersistConfig {
            path: self.paths.config_path.clone(),
            source: source.into(),
        })
    }

    pub(crate) fn apply(&self, config: &Config) -> Result<(), BootstrapConfigError> {
        let resolved = ResolvedConfig::try_from(config)?;

        koharu_runtime::http::set_download_settings(DownloadSettings {
            proxy_url: resolved.proxy_url.as_ref().map(Url::to_string),
            pypi_mirror: resolved.pypi_mirror.to_wire(),
            github_mirror: resolved.github_mirror.to_wire(),
        })?;
        Ok(())
    }

    pub(crate) fn dependency_paths(
        &self,
        config: &Config,
    ) -> Result<BootstrapPaths, BootstrapConfigError> {
        let resolved = ResolvedConfig::try_from(config)?;
        Ok(BootstrapPaths::new(
            resolved.runtime_root.into_std_path_buf(),
            resolved.models_root.into_std_path_buf(),
        ))
    }

    pub(crate) fn enforce_locked_paths(
        &self,
        previous: &Config,
        next: &Config,
    ) -> Result<(), BootstrapConfigError> {
        if previous.runtime_path != next.runtime_path {
            return Err(BootstrapConfigError::RuntimePathLocked);
        }
        if previous.models_path != next.models_path {
            return Err(BootstrapConfigError::ModelsPathLocked);
        }
        Ok(())
    }
}

fn ensure_dir(path: &Path, display: &Utf8PathBuf) -> Result<(), BootstrapConfigError> {
    std::fs::create_dir_all(path).map_err(|source| BootstrapConfigError::CreateDir {
        path: display.clone(),
        source,
    })
}

fn utf8_path(path: PathBuf) -> Result<Utf8PathBuf, BootstrapConfigError> {
    Utf8PathBuf::from_path_buf(path.clone()).map_err(|_| BootstrapConfigError::NonUtf8Path(path))
}

fn default_config(paths: &ProjectPaths) -> Config {
    Config {
        language: "en-US".to_string(),
        runtime_path: paths.default_runtime_root.to_string(),
        models_path: paths.default_models_root.to_string(),
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

#[derive(Debug, Clone, TypedBuilder)]
struct ResolvedConfig {
    runtime_root: Utf8PathBuf,
    models_root: Utf8PathBuf,
    #[builder(default)]
    proxy_url: Option<Url>,
    pypi_mirror: ResolvedMirrorSelection,
    github_mirror: ResolvedMirrorSelection,
}

impl TryFrom<&Config> for ResolvedConfig {
    type Error = BootstrapConfigError;

    fn try_from(value: &Config) -> Result<Self, Self::Error> {
        Ok(Self::builder()
            .runtime_root(parse_required_path(
                &value.runtime_path,
                BootstrapConfigError::MissingRuntimePath,
            )?)
            .models_root(parse_required_path(
                &value.models_path,
                BootstrapConfigError::MissingModelsPath,
            )?)
            .proxy_url(parse_optional_url(value.proxy_url.as_deref(), "proxy")?)
            .pypi_mirror(ResolvedMirrorSelection::try_from((
                "pypi",
                &value.pypi_mirror,
            ))?)
            .github_mirror(ResolvedMirrorSelection::try_from((
                "github",
                &value.github_mirror,
            ))?)
            .build())
    }
}

#[derive(Debug, Clone, TypedBuilder)]
struct ResolvedMirrorSelection {
    kind: MirrorKind,
    #[builder(default)]
    custom_base_url: Option<Url>,
}

impl ResolvedMirrorSelection {
    fn to_wire(&self) -> MirrorSelection {
        MirrorSelection {
            kind: self.kind,
            custom_base_url: self.custom_base_url.as_ref().map(Url::to_string),
        }
    }
}

impl TryFrom<(&'static str, &MirrorSelection)> for ResolvedMirrorSelection {
    type Error = BootstrapConfigError;

    fn try_from(value: (&'static str, &MirrorSelection)) -> Result<Self, Self::Error> {
        let (field, mirror) = value;
        let custom_base_url = parse_optional_url(mirror.custom_base_url.as_deref(), field)?;
        if matches!(mirror.kind, MirrorKind::Custom) && custom_base_url.is_none() {
            return Err(BootstrapConfigError::MissingMirrorUrl { field });
        }

        Ok(Self::builder()
            .kind(mirror.kind)
            .custom_base_url(custom_base_url)
            .build())
    }
}

fn parse_required_path(
    value: &str,
    missing: BootstrapConfigError,
) -> Result<Utf8PathBuf, BootstrapConfigError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(missing);
    }
    Ok(Utf8PathBuf::from(value))
}

fn parse_optional_url(
    value: Option<&str>,
    field: &'static str,
) -> Result<Option<Url>, BootstrapConfigError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    Url::parse(value).map(Some).map_err(|source| match field {
        "proxy" => BootstrapConfigError::InvalidProxyUrl {
            value: value.to_string(),
            source,
        },
        _ => BootstrapConfigError::InvalidMirrorUrl {
            field,
            value: value.to_string(),
            source,
        },
    })
}

fn parse_config(bytes: &[u8]) -> Result<Config, BootstrapConfigError> {
    let mut deserializer = serde_json::Deserializer::from_reader(Cursor::new(bytes));
    serde_path_to_error::deserialize::<_, Config>(&mut deserializer).map_err(|error| {
        BootstrapConfigError::ParseConfig {
            path: path_to_string(error.path().iter()),
            source: error.into_inner(),
        }
    })
}

fn path_to_string<'a>(segments: impl Iterator<Item = &'a Segment>) -> String {
    let mut rendered = String::new();
    for segment in segments {
        match segment {
            Segment::Seq { index } => rendered.push_str(&format!("[{index}]")),
            Segment::Map { key } => {
                if !rendered.is_empty() {
                    rendered.push('.');
                }
                rendered.push_str(key);
            }
            Segment::Enum { variant } => {
                if !rendered.is_empty() {
                    rendered.push('.');
                }
                rendered.push_str(variant);
            }
            Segment::Unknown => rendered.push_str(".<unknown>"),
        }
    }
    if rendered.is_empty() {
        "<root>".to_string()
    } else {
        rendered
    }
}

#[cfg(test)]
mod tests {
    use koharu_core::{Config, MirrorKind, MirrorSelection};

    use super::{BootstrapConfigError, ProjectPaths, ResolvedConfig, default_config};

    fn sample_config() -> Config {
        default_config(
            &ProjectPaths::builder()
                .app_root("C:/koharu".into())
                .config_path("C:/koharu/config.json".into())
                .default_runtime_root("C:/koharu/runtime".into())
                .default_models_root("C:/koharu/models".into())
                .build(),
        )
    }

    #[test]
    fn rejects_blank_paths() {
        let mut config = sample_config();
        config.runtime_path = "   ".to_string();
        assert!(matches!(
            ResolvedConfig::try_from(&config),
            Err(BootstrapConfigError::MissingRuntimePath)
        ));

        let mut config = sample_config();
        config.models_path = "".to_string();
        assert!(matches!(
            ResolvedConfig::try_from(&config),
            Err(BootstrapConfigError::MissingModelsPath)
        ));
    }

    #[test]
    fn rejects_custom_mirror_without_url() {
        let mut config = sample_config();
        config.pypi_mirror = MirrorSelection {
            kind: MirrorKind::Custom,
            custom_base_url: Some("   ".to_string()),
        };

        assert!(matches!(
            ResolvedConfig::try_from(&config),
            Err(BootstrapConfigError::MissingMirrorUrl { field: "pypi" })
        ));
    }
}
