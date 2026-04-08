use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use reqwest_middleware::ClientWithMiddleware;
use tokio::sync::broadcast;

use crate::downloads::Downloads;
use crate::packages::PackageCatalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputePolicy {
    PreferGpu,
    CpuOnly,
}

#[derive(Debug, Clone)]
pub struct RuntimeHttpConfig {
    pub connect_timeout_secs: u64,
    pub read_timeout_secs: u64,
    pub max_retries: u32,
}

impl Default for RuntimeHttpConfig {
    fn default() -> Self {
        Self {
            connect_timeout_secs: 20,
            read_timeout_secs: 300,
            max_retries: 3,
        }
    }
}

pub fn default_app_data_root() -> Utf8PathBuf {
    let root = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Koharu");
    Utf8PathBuf::from_path_buf(root)
        .unwrap_or_else(|path| Utf8PathBuf::from(path.to_string_lossy().into_owned()))
}

#[derive(Clone)]
pub struct Runtime {
    inner: Arc<RuntimeInner>,
}

struct RuntimeInner {
    root: PathBuf,
    compute: ComputePolicy,
    downloads: Downloads,
    packages: PackageCatalog,
}

impl Runtime {
    pub fn new(root: impl Into<PathBuf>, compute: ComputePolicy) -> Result<Self> {
        Self::new_with_http(root, compute, RuntimeHttpConfig::default())
    }

    pub fn new_with_http(
        root: impl Into<PathBuf>,
        compute: ComputePolicy,
        http: RuntimeHttpConfig,
    ) -> Result<Self> {
        let root = root.into();
        let downloads = Downloads::new(
            root.join("runtime").join(".downloads"),
            root.join("models").join("huggingface"),
            &http,
        )?;

        Ok(Self {
            inner: Arc::new(RuntimeInner {
                root,
                compute,
                downloads,
                packages: PackageCatalog::discover(),
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    pub fn wants_gpu(&self) -> bool {
        matches!(self.inner.compute, ComputePolicy::PreferGpu)
    }

    pub fn http_client(&self) -> Arc<ClientWithMiddleware> {
        self.inner.downloads.client()
    }

    pub fn subscribe_downloads(&self) -> broadcast::Receiver<koharu_core::DownloadProgress> {
        self.inner.downloads.subscribe()
    }

    pub fn downloads(&self) -> Downloads {
        self.inner.downloads.clone()
    }

    pub async fn prepare(&self) -> Result<()> {
        let dirs = [
            self.root().join("runtime"),
            self.root().join("runtime").join(".downloads"),
            self.root().join("models"),
            self.root().join("models").join("huggingface"),
        ];
        for dir in dirs {
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("failed to create `{}`", dir.display()))?;
        }
        self.inner.packages.prepare_bootstrap(self).await
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
        let runtime = Runtime::new(tempdir.path(), ComputePolicy::CpuOnly)?;
        runtime.prepare().await?;
        assert!(runtime.llama_directory()?.exists());
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn repeated_basename_loads_succeed_after_prepare() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let runtime = Runtime::new(tempdir.path(), ComputePolicy::CpuOnly)?;
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
