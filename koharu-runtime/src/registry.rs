use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::future::BoxFuture;
use once_cell::sync::Lazy;

pub type EnsureRuntime = fn(PathBuf) -> BoxFuture<'static, Result<()>>;
pub type RuntimeReady = fn(&Path) -> Result<bool>;

#[derive(Clone)]
pub struct BootstrapPaths {
    pub runtime_root: PathBuf,
    pub models_root: PathBuf,
}

impl BootstrapPaths {
    pub fn new(runtime_root: impl Into<PathBuf>, models_root: impl Into<PathBuf>) -> Self {
        Self {
            runtime_root: runtime_root.into(),
            models_root: models_root.into(),
        }
    }
}

#[derive(Clone)]
pub enum BootstrapSource {
    Runtime {
        ready: RuntimeReady,
        ensure: EnsureRuntime,
    },
    ModelAsset {
        repo: &'static str,
        filename: &'static str,
    },
}

#[derive(Clone)]
pub struct BootstrapEntry {
    pub id: String,
    pub label: String,
    pub priority: u32,
    pub required: bool,
    pub source: BootstrapSource,
}

impl BootstrapEntry {
    pub fn runtime(
        id: impl Into<String>,
        label: impl Into<String>,
        priority: u32,
        required: bool,
        ready: RuntimeReady,
        ensure: EnsureRuntime,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            priority,
            required,
            source: BootstrapSource::Runtime { ready, ensure },
        }
    }

    pub fn model(
        id: impl Into<String>,
        label: impl Into<String>,
        priority: u32,
        required: bool,
        repo: &'static str,
        filename: &'static str,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            priority,
            required,
            source: BootstrapSource::ModelAsset { repo, filename },
        }
    }

    pub fn is_ready(&self, paths: &BootstrapPaths) -> Result<bool> {
        match &self.source {
            BootstrapSource::Runtime { ready, .. } => ready(&paths.runtime_root),
            BootstrapSource::ModelAsset { repo, filename } => {
                Ok(crate::download::cached_model_path(&paths.models_root, repo, filename).is_ok())
            }
        }
    }

    pub async fn ensure(&self, paths: &BootstrapPaths) -> Result<()> {
        match &self.source {
            BootstrapSource::Runtime { ensure, .. } => ensure(paths.runtime_root.clone()).await,
            BootstrapSource::ModelAsset { repo, filename } => {
                crate::download::model(&paths.models_root, repo, filename)
                    .await
                    .map(|_| ())
            }
        }
    }

    pub fn repo(&self) -> Option<&'static str> {
        match &self.source {
            BootstrapSource::Runtime { .. } => None,
            BootstrapSource::ModelAsset { repo, .. } => Some(repo),
        }
    }

    pub fn filename(&self) -> Option<&'static str> {
        match &self.source {
            BootstrapSource::Runtime { .. } => None,
            BootstrapSource::ModelAsset { filename, .. } => Some(filename),
        }
    }
}

pub struct RegistryProvider {
    pub entries: fn() -> Vec<BootstrapEntry>,
}

inventory::collect!(RegistryProvider);

static REGISTRY: Lazy<Vec<BootstrapEntry>> = Lazy::new(|| {
    let mut entries = inventory::iter::<RegistryProvider>
        .into_iter()
        .flat_map(|provider| (provider.entries)())
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    entries
});

static MODEL_LOOKUP: Lazy<HashMap<(String, String), BootstrapEntry>> = Lazy::new(|| {
    REGISTRY
        .iter()
        .filter_map(|entry| {
            Some((
                (entry.repo()?.to_string(), entry.filename()?.to_string()),
                entry.clone(),
            ))
        })
        .collect()
});

pub fn entries() -> &'static [BootstrapEntry] {
    &REGISTRY
}

pub fn required_entries() -> impl Iterator<Item = &'static BootstrapEntry> {
    REGISTRY.iter().filter(|entry| entry.required)
}

pub fn lookup_model(repo: &str, filename: &str) -> Option<BootstrapEntry> {
    MODEL_LOOKUP
        .get(&(repo.to_string(), filename.to_string()))
        .cloned()
}
