use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    #[serde(default = "DirectorySetting::runtime_default")]
    pub runtime: DirectorySetting,
    #[serde(default = "DirectorySetting::models_default")]
    pub models: DirectorySetting,
    pub http: HttpSetting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectorySetting {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct HttpSetting {
    pub proxy: Option<Url>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputePolicy {
    PreferGpu,
    CpuOnly,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsBuilder {
    runtime_root: Option<PathBuf>,
    models_root: Option<PathBuf>,
    proxy: Option<Url>,
}

pub type PathSetting = DirectorySetting;

impl Default for Settings {
    fn default() -> Self {
        Self {
            runtime: DirectorySetting::runtime_default(),
            models: DirectorySetting::models_default(),
            http: HttpSetting::default(),
        }
    }
}

impl Settings {
    pub fn builder() -> SettingsBuilder {
        SettingsBuilder::default()
    }

    pub fn from_paths(runtime_root: impl Into<PathBuf>, models_root: impl Into<PathBuf>) -> Self {
        Self::builder()
            .runtime_root(runtime_root)
            .models_root(models_root)
            .build()
    }

    pub fn with_runtime_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.runtime = DirectorySetting::new(path);
        self
    }

    pub fn with_models_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.models = DirectorySetting::new(path);
        self
    }

    pub fn with_proxy(mut self, proxy: Option<Url>) -> Self {
        self.http.proxy = proxy;
        self
    }

    pub fn runtime_root(&self) -> &Path {
        &self.runtime.path
    }

    pub fn models_root(&self) -> &Path {
        &self.models.path
    }

    pub fn http_proxy(&self) -> Option<&Url> {
        self.http.proxy.as_ref()
    }

    pub(crate) fn apply_process_overrides(mut self) -> Self {
        if let Some(path) = env::var_os("KOHARU_RUNTIME_ROOT") {
            self.runtime.path = PathBuf::from(path);
        }
        self
    }
}

impl ComputePolicy {
    pub fn wants_gpu(self) -> bool {
        matches!(self, Self::PreferGpu)
    }
}

impl SettingsBuilder {
    pub fn runtime_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.runtime_root = Some(path.into());
        self
    }

    pub fn models_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.models_root = Some(path.into());
        self
    }

    pub fn proxy(mut self, proxy: impl Into<Option<Url>>) -> Self {
        self.proxy = proxy.into();
        self
    }

    pub fn build(self) -> Settings {
        Settings {
            runtime: DirectorySetting::new(self.runtime_root.unwrap_or_else(default_runtime_root)),
            models: DirectorySetting::new(self.models_root.unwrap_or_else(default_models_root)),
            http: HttpSetting { proxy: self.proxy },
        }
    }
}

impl DirectorySetting {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn runtime_default() -> Self {
        Self::new(default_runtime_root())
    }

    fn models_default() -> Self {
        Self::new(default_models_root())
    }
}

pub fn default_runtime_root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(env::temp_dir)
        .join("Koharu")
        .join("runtime")
}

pub fn default_models_root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(env::temp_dir)
        .join("Koharu")
        .join("models")
}
