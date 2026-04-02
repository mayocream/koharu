use std::path::PathBuf;
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

impl Runtime {
    pub fn new(settings: Settings, compute: ComputePolicy) -> Result<Self> {
        let layout = Layout::from_settings(&settings);
        let http = HttpStack::new()?;
        let transfers = TransferHub::new();
        let artifacts = ArtifactStore::new(layout.clone(), http.clone(), transfers.clone());

        Ok(Self {
            inner: Arc::new(RuntimeInner {
                settings,
                compute,
                layout,
                http,
                transfers,
                artifacts,
                packages: PackageCatalog::discover(),
            }),
        })
    }

    pub fn settings(&self) -> &Settings {
        &self.inner.settings
    }

    pub fn layout(&self) -> &Layout {
        &self.inner.layout
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

    pub async fn prepare(&self) -> Result<()> {
        self.layout().ensure_roots()?;
        self.catalog().prepare_bootstrap(self).await
    }

    pub fn llama_directory(&self) -> Result<PathBuf> {
        crate::llama::runtime_dir(self)
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
            Settings {
                runtime: crate::DirectorySetting {
                    path: camino::Utf8PathBuf::from_path_buf(tempdir.path().join("runtime"))
                        .unwrap(),
                },
                models: crate::DirectorySetting {
                    path: camino::Utf8PathBuf::from_path_buf(tempdir.path().join("models"))
                        .unwrap(),
                },
            },
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
            Settings {
                runtime: crate::DirectorySetting {
                    path: camino::Utf8PathBuf::from_path_buf(tempdir.path().join("runtime"))
                        .unwrap(),
                },
                models: crate::DirectorySetting {
                    path: camino::Utf8PathBuf::from_path_buf(tempdir.path().join("models"))
                        .unwrap(),
                },
            },
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
