mod archive;
mod cuda;
mod llama;
mod loader;

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
    let _root_lock = koharu_http::lock::acquire_managed_root(root)?;
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

pub fn runtime_root() -> PathBuf {
    koharu_http::paths::runtime_root()
}

pub fn delete_runtime_root() -> Result<()> {
    let root = runtime_root();
    let _root_lock = koharu_http::lock::acquire_managed_root(&root)?;
    if root.exists() {
        fs::remove_dir_all(&root)
            .with_context(|| format!("failed to delete runtime root `{}`", root.display()))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeValidation {
    Missing,
    Ready,
    Partial,
    FailedValidation,
    Busy,
}

pub fn validate_runtime() -> RuntimeValidation {
    let root = runtime_root();
    let Ok(_root_lock) = koharu_http::lock::acquire_managed_root(&root) else {
        return RuntimeValidation::Busy;
    };

    let downloads_dir = root.join(DOWNLOADS_DIR);
    let mut any_present = downloads_dir.exists();
    let mut any_invalid = false;

    let Some(llama_install_dir) = llama::runtime_install_dir_for_current_platform(&root) else {
        return RuntimeValidation::FailedValidation;
    };
    let llama_libraries = llama::required_libraries_for_current_platform();
    let llama_marker = llama_install_dir.join(INSTALLED_MARKER);
    let llama_present = llama_install_dir.exists();
    any_present |= llama_present;

    if llama_present {
        let libraries_ok = llama_libraries
            .iter()
            .all(|library| file_exists_and_non_empty(&llama_install_dir.join(library)));
        let marker_ok = file_exists_and_non_empty(&llama_marker);
        if !libraries_ok || !marker_ok {
            any_invalid = true;
        }
    }

    if let Some(cuda_install_dir) = cuda::runtime_install_dir_if_applicable(&root) {
        let cuda_marker = cuda_install_dir.join(INSTALLED_MARKER);
        let cuda_present = cuda_install_dir.exists();
        any_present |= cuda_present;
        if cuda_present {
            let libraries_ok = cuda::required_libraries_for_current_platform()
                .iter()
                .all(|library| file_exists_and_non_empty(&cuda_install_dir.join(library)));
            let marker_ok = file_exists_and_non_empty(&cuda_marker);
            if !libraries_ok || !marker_ok {
                any_invalid = true;
            }
        }
    }

    if !any_present {
        RuntimeValidation::Missing
    } else if any_invalid {
        RuntimeValidation::FailedValidation
    } else if file_exists_and_non_empty(&llama_marker) {
        RuntimeValidation::Ready
    } else {
        RuntimeValidation::Partial
    }
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

fn file_exists_and_non_empty(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
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
