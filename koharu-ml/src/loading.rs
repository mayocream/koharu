use std::future::Future;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use candle_core::{DType, Device};
use candle_nn::VarBuilder;
use serde::de::DeserializeOwned;

pub async fn resolve_manifest_path<F>(manifest: F) -> Result<PathBuf>
where
    F: Future<Output = Result<PathBuf>>,
{
    manifest.await
}

pub async fn load_mmaped_safetensors<F, T, Build, E>(
    manifest: F,
    device: &Device,
    build: Build,
) -> Result<T>
where
    F: Future<Output = Result<PathBuf>>,
    Build: FnOnce(VarBuilder) -> std::result::Result<T, E>,
    E: Into<anyhow::Error>,
{
    let weights = resolve_manifest_path(manifest).await?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, device)? };
    build(vb).map_err(Into::into)
}

pub async fn load_buffered_safetensors<F, T, Build, E>(
    manifest: F,
    device: &Device,
    build: Build,
) -> Result<T>
where
    F: Future<Output = Result<PathBuf>>,
    Build: FnOnce(VarBuilder) -> std::result::Result<T, E>,
    E: Into<anyhow::Error>,
{
    let weights = resolve_manifest_path(manifest).await?;
    let data =
        std::fs::read(&weights).with_context(|| format!("failed to read {}", weights.display()))?;
    let vb = VarBuilder::from_buffered_safetensors(data, DType::F32, device)?;
    build(vb).map_err(Into::into)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str(&data)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed)
}
