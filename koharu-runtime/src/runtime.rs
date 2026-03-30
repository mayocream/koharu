use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use reqwest_middleware::ClientWithMiddleware;
use tokio::sync::broadcast;

use crate::artifacts::ArtifactStore;
use crate::downloads::TransferHub;
use crate::http::HttpStack;
use crate::layout::Layout;
use crate::packages::PackageCatalog;
use crate::{ComputePolicy, Settings};

#[derive(Clone)]
pub struct Runtime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    settings: Settings,
    compute: ComputePolicy,
    layout: Layout,
    http: HttpStack,
    transfers: TransferHub,
    artifacts: ArtifactStore,
    packages: PackageCatalog,
}

#[derive(Debug, Clone)]
pub struct RuntimeBuilder {
    settings: Settings,
    compute: ComputePolicy,
}

impl Runtime {
    pub fn builder(settings: Settings) -> RuntimeBuilder {
        RuntimeBuilder::new(settings)
    }

    pub fn new(settings: Settings, compute: ComputePolicy) -> Result<Self> {
        RuntimeBuilder::new(settings)
            .compute_policy(compute)
            .build()
    }

    pub fn settings(&self) -> &Settings {
        &self.inner.settings
    }

    pub fn layout(&self) -> &Layout {
        &self.inner.layout
    }

    pub fn runtime_root(&self) -> &Path {
        self.layout().runtime_root()
    }

    pub fn models_root(&self) -> &Path {
        self.layout().models_root()
    }

    pub fn downloads_root(&self) -> PathBuf {
        self.layout().downloads_root().to_path_buf()
    }

    pub fn http_proxy(&self) -> Option<&url::Url> {
        self.settings().http_proxy()
    }

    pub fn wants_gpu(&self) -> bool {
        self.inner.compute.wants_gpu()
    }

    pub fn http_client(&self) -> Arc<ClientWithMiddleware> {
        self.inner.http.client()
    }

    pub fn subscribe_downloads(&self) -> broadcast::Receiver<koharu_core::DownloadProgress> {
        self.inner.transfers.subscribe()
    }

    pub fn artifacts(&self) -> ArtifactStore {
        self.inner.artifacts.clone()
    }

    pub fn catalog(&self) -> &PackageCatalog {
        &self.inner.packages
    }

    pub fn needs_bootstrap(&self) -> Result<bool> {
        self.catalog().requires_bootstrap(self)
    }

    pub async fn prepare(&self) -> Result<()> {
        self.layout().ensure_roots()?;
        self.catalog().prepare_bootstrap(self).await
    }

    pub fn llama_directory(&self) -> Result<PathBuf> {
        crate::llama::runtime_dir(self)
    }
}

impl RuntimeBuilder {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            compute: ComputePolicy::PreferGpu,
        }
    }

    pub fn compute_policy(mut self, compute: ComputePolicy) -> Self {
        self.compute = compute;
        self
    }

    pub fn cpu_only(self) -> Self {
        self.compute_policy(ComputePolicy::CpuOnly)
    }

    pub fn build(self) -> Result<Runtime> {
        let settings = self.settings.apply_process_overrides();
        let layout = Layout::from_settings(&settings);
        let http = HttpStack::new(&settings)?;
        let transfers = TransferHub::new();
        let artifacts = ArtifactStore::new(layout.clone(), http.clone(), transfers.clone());

        Ok(Runtime {
            inner: Arc::new(RuntimeInner {
                settings,
                compute: self.compute,
                layout,
                http,
                transfers,
                artifacts,
                packages: PackageCatalog::discover(),
            }),
        })
    }
}

pub type RuntimeManager = Runtime;

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Result;

    use super::*;

    #[tokio::test]
    #[ignore]
    async fn prepares_llama_runtime_into_configured_root() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let runtime = Runtime::new(
            Settings::from_paths(
                tempdir.path().join("runtime"),
                tempdir.path().join("models"),
            ),
            ComputePolicy::CpuOnly,
        )?;
        runtime.prepare().await?;
        assert!(runtime.llama_directory()?.exists());
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn repeated_basename_loads_succeed_after_prepare() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let runtime = Runtime::new(
            Settings::from_paths(
                tempdir.path().join("runtime"),
                tempdir.path().join("models"),
            ),
            ComputePolicy::CpuOnly,
        )?;
        runtime.prepare().await?;
        let dir = runtime.llama_directory()?;

        let lib_name = fs::read_dir(&dir)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.contains("llama").then_some(name)
            })
            .next()
            .ok_or_else(|| anyhow::anyhow!("no llama library found"))?;

        let _first = crate::load_library_by_name(&lib_name)?;
        let _second = crate::load_library_by_name(&lib_name)?;
        Ok(())
    }
}
