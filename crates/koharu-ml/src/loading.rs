use std::future::Future;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use candle_core::{DType, Device};
use candle_nn::VarBuilder;
use serde::de::DeserializeOwned;

pub fn model_dtype(device: &Device) -> DType {
    crate::ops::model_dtype(device)
}

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
    load_mmaped_safetensors_path(&weights, device, build)
}

pub fn load_mmaped_safetensors_path<T, Build, E>(
    weights: &Path,
    device: &Device,
    build: Build,
) -> Result<T>
where
    Build: FnOnce(VarBuilder) -> std::result::Result<T, E>,
    E: Into<anyhow::Error>,
{
    load_mmaped_safetensors_path_with_dtype(weights, device, DType::F32, build)
}

pub fn load_mmaped_safetensors_path_with_dtype<T, Build, E>(
    weights: &Path,
    device: &Device,
    dtype: DType,
    build: Build,
) -> Result<T>
where
    Build: FnOnce(VarBuilder) -> std::result::Result<T, E>,
    E: Into<anyhow::Error>,
{
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], dtype, device)? };
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
    load_buffered_safetensors_path(&weights, device, build)
}

pub fn load_buffered_safetensors_path<T, Build, E>(
    weights: &Path,
    device: &Device,
    build: Build,
) -> Result<T>
where
    Build: FnOnce(VarBuilder) -> std::result::Result<T, E>,
    E: Into<anyhow::Error>,
{
    load_buffered_safetensors_path_with_dtype(weights, device, DType::F32, build)
}

pub fn load_buffered_safetensors_path_with_dtype<T, Build, E>(
    weights: &Path,
    device: &Device,
    dtype: DType,
    build: Build,
) -> Result<T>
where
    Build: FnOnce(VarBuilder) -> std::result::Result<T, E>,
    E: Into<anyhow::Error>,
{
    let data =
        std::fs::read(weights).with_context(|| format!("failed to read {}", weights.display()))?;
    let vb = VarBuilder::from_buffered_safetensors(data, dtype, device)?;
    build(vb).map_err(Into::into)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str(&data)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed)
}
