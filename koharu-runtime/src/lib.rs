mod archive;
mod cuda;
mod llama;
mod loader;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub use cuda::{CudaDriverVersion, driver_version as cuda_driver_version};
pub use loader::{load_library_by_name, load_library_by_path};

const DOWNLOADS_DIR: &str = ".downloads";
const INSTALLED_MARKER: &str = ".installed";

pub async fn initialize() -> Result<()> {
    initialize_with_root(&runtime_root()).await
}

async fn initialize_with_root(root: &Path) -> Result<()> {
    fs::create_dir_all(root)
        .with_context(|| format!("failed to create runtime root `{}`", root.display()))?;
    let downloads_dir = root.join(DOWNLOADS_DIR);
    fs::create_dir_all(&downloads_dir)
        .with_context(|| format!("failed to create `{}`", downloads_dir.display()))?;

    if cuda::is_available() {
        cuda::ensure_ready(root, &downloads_dir).await?;
    }
    llama::ensure_ready(root, &downloads_dir).await?;

    Ok(())
}

#[doc(hidden)]
pub fn llama_runtime_dir() -> Result<PathBuf> {
    llama::runtime_dir(&runtime_root())
}

fn runtime_root() -> PathBuf {
    if let Some(path) = env::var_os("KOHARU_RUNTIME_ROOT") {
        return PathBuf::from(path);
    }
    dirs::data_local_dir()
        .unwrap_or_else(env::temp_dir)
        .join("Koharu")
        .join("runtime")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn initializes_llama_runtime_into_shared_root() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        initialize_with_root(tempdir.path()).await?;
        let dir = llama::runtime_dir(tempdir.path())?;
        assert!(dir.exists());
        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn repeated_basename_loads_succeed_after_runtime_initialize() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        initialize_with_root(tempdir.path()).await?;
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
