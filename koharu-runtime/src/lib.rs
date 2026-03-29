mod archive;
mod cuda;
pub mod download;
mod hf_hub;
pub mod http;
mod llama;
mod loader;
mod progress;
mod range;
pub mod registry;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub use cuda::{CudaDriverVersion, driver_version as cuda_driver_version};
pub use loader::{load_library_by_name, load_library_by_path};
pub use registry::{BootstrapEntry, BootstrapPaths};

const DOWNLOADS_DIR: &str = ".downloads";
const INSTALLED_MARKER: &str = ".installed";

pub async fn initialize(runtime_root: &Path) -> Result<()> {
    initialize_with_root(runtime_root).await
}

pub async fn ensure_cuda_runtime(runtime_root: &Path) -> Result<()> {
    let downloads_dir = prepare_runtime_root(runtime_root)?;
    if cuda::is_available() {
        cuda::ensure_ready(runtime_root, &downloads_dir).await?;
    }
    Ok(())
}

pub fn cuda_runtime_ready(runtime_root: &Path) -> Result<bool> {
    if cuda::is_available() {
        return cuda::is_ready(runtime_root);
    }
    Ok(true)
}

pub async fn ensure_llama_runtime(runtime_root: &Path) -> Result<()> {
    let downloads_dir = prepare_runtime_root(runtime_root)?;
    llama::ensure_ready(runtime_root, &downloads_dir).await
}

pub fn llama_runtime_ready(runtime_root: &Path) -> Result<bool> {
    llama::is_ready(runtime_root)
}

async fn initialize_with_root(root: &Path) -> Result<()> {
    let downloads_dir = prepare_runtime_root(root)?;

    if cuda::is_available() {
        cuda::ensure_ready(root, &downloads_dir).await?;
    }
    llama::ensure_ready(root, &downloads_dir).await?;

    Ok(())
}

#[doc(hidden)]
pub fn llama_runtime_dir(runtime_root: &Path) -> Result<PathBuf> {
    llama::runtime_dir(runtime_root)
}

fn prepare_runtime_root(root: &Path) -> Result<PathBuf> {
    fs::create_dir_all(root)
        .with_context(|| format!("failed to create runtime root `{}`", root.display()))?;
    let downloads_dir = root.join(DOWNLOADS_DIR);
    fs::create_dir_all(&downloads_dir)
        .with_context(|| format!("failed to create `{}`", downloads_dir.display()))?;
    Ok(downloads_dir)
}

fn is_up_to_date(install_dir: &Path, source_id: &str) -> bool {
    matches!(
        fs::read_to_string(install_dir.join(INSTALLED_MARKER)),
        Ok(content) if content == source_id
    )
}

fn reset_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("failed to reset `{}`", dir.display()))?;
    }
    fs::create_dir_all(dir).with_context(|| format!("failed to create `{}`", dir.display()))?;
    Ok(())
}

fn mark_installed(install_dir: &Path, source_id: &str) -> Result<()> {
    fs::write(install_dir.join(INSTALLED_MARKER), source_id)
        .with_context(|| format!("failed to write marker in `{}`", install_dir.display()))
}

fn register_runtime_entries() -> Vec<registry::BootstrapEntry> {
    vec![
        registry::BootstrapEntry::runtime(
            "cuda-runtime",
            "CUDA runtime",
            100,
            true,
            cuda_runtime_ready,
            |runtime_root| Box::pin(async move { ensure_cuda_runtime(&runtime_root).await }),
        ),
        registry::BootstrapEntry::runtime(
            "llama-runtime",
            "llama.cpp runtime",
            200,
            true,
            llama_runtime_ready,
            |runtime_root| Box::pin(async move { ensure_llama_runtime(&runtime_root).await }),
        ),
    ]
}

inventory::submit! {
    registry::RegistryProvider {
        entries: register_runtime_entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn initializes_llama_runtime_into_shared_root() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        initialize(tempdir.path()).await?;
        let dir = llama::runtime_dir(tempdir.path())?;
        assert!(dir.exists());
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn repeated_basename_loads_succeed_after_runtime_initialize() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        initialize(tempdir.path()).await?;
        let dir = llama::runtime_dir(tempdir.path())?;

        let lib_name = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.contains("llama").then_some(name)
            })
            .next()
            .ok_or_else(|| anyhow::anyhow!("no llama library found"))?;

        let _first = load_library_by_name(&lib_name)?;
        let _second = load_library_by_name(&lib_name)?;
        Ok(())
    }
}
