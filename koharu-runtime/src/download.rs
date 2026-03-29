use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use anyhow::Context;
use futures::{StreamExt, TryStreamExt, stream};
use koharu_core::{DownloadProgress, DownloadStatus};
use tokio::sync::broadcast;
use tracing::Instrument;

use crate::hf_hub;
use crate::http;
use crate::progress::progress_bar;
use crate::range;

const RANGE_CHUNK_SIZE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct DownloadDescriptor {
    pub id: String,
    pub label: String,
    pub filename: String,
}

static TX: LazyLock<broadcast::Sender<DownloadProgress>> =
    LazyLock::new(|| broadcast::channel(1024).0);

pub fn subscribe() -> broadcast::Receiver<DownloadProgress> {
    TX.subscribe()
}

pub(crate) fn emit(progress: DownloadProgress) {
    let _ = TX.send(progress);
}

pub async fn model(
    cache_dir: impl AsRef<Path>,
    repo: &str,
    filename: &str,
) -> anyhow::Result<PathBuf> {
    let cache_dir = cache_dir.as_ref();
    let descriptor = lookup_model_descriptor(repo, filename);
    let hf_repo = hf_hub::repo(repo);

    if let Some(path) = hf_hub::cache(cache_dir).repo(hf_repo.clone()).get(filename) {
        return Ok(path);
    }

    let reporter = hf_hub::Reporter::new(descriptor.clone());

    let path = hf_hub::api(cache_dir)
        .repo(hf_repo)
        .download_with_progress(filename, reporter)
        .instrument(tracing::info_span!("hf_download", repo, filename))
        .await
        .context("failed to download from HF Hub")?;

    Ok(path)
}

pub fn cached_model_path(
    cache_dir: impl AsRef<Path>,
    repo: &str,
    filename: &str,
) -> anyhow::Result<PathBuf> {
    hf_hub::cache(cache_dir.as_ref())
        .repo(hf_hub::repo(repo))
        .get(filename)
        .ok_or_else(|| anyhow::anyhow!("model asset not found in cache: {repo}/{filename}"))
}

#[tracing::instrument(level = "info")]
pub async fn bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    bytes_with_descriptor(url, descriptor_from_url(url)).await
}

#[tracing::instrument(level = "info", skip(descriptor))]
pub async fn bytes_with_descriptor(
    url: &str,
    descriptor: DownloadDescriptor,
) -> anyhow::Result<Vec<u8>> {
    let resolved_url = http::rewrite_url(url);
    let head = range::head(&resolved_url)
        .await
        .with_context(|| format!("cannot download {url}"))?;
    let total_bytes = head.content_length;

    anyhow::ensure!(total_bytes > 0, "resource reports zero Content-Length");
    anyhow::ensure!(
        head.supports_ranges,
        "remote server does not advertise byte ranges"
    );

    let pb = Arc::new(progress_bar(&descriptor.filename));
    let total_len =
        usize::try_from(total_bytes).context("resource too large to fit into memory")?;
    let chunk_size = total_len.clamp(1, RANGE_CHUNK_SIZE_BYTES);
    let segments = total_len.div_ceil(chunk_size);

    pb.set_length(total_len as u64);
    emit_progress(&descriptor, 0, Some(total_bytes), DownloadStatus::Started);

    tracing::debug!(
        url,
        resolved_url,
        total_bytes,
        segments,
        "downloading resource via HTTP range requests"
    );

    let downloaded = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let chunks = {
        let url = Arc::new(resolved_url);
        let pb = Arc::clone(&pb);
        let downloaded = Arc::clone(&downloaded);
        let descriptor = descriptor.clone();
        stream::iter((0..segments).map(move |index| {
            let start = (index * chunk_size) as u64;
            let len = ((index + 1) * chunk_size).min(total_len) - (index * chunk_size);
            let end = start + len as u64 - 1;
            let url = Arc::clone(&url);
            let pb = Arc::clone(&pb);
            let downloaded = Arc::clone(&downloaded);
            let descriptor = descriptor.clone();
            async move {
                let chunk = range::get_range(&url, start, end).await?;
                pb.inc(len as u64);
                let current = downloaded
                    .fetch_add(len as u64, std::sync::atomic::Ordering::Relaxed)
                    + len as u64;
                emit_progress(
                    &descriptor,
                    current,
                    Some(total_bytes),
                    DownloadStatus::Downloading,
                );
                Ok::<_, anyhow::Error>((start, chunk))
            }
        }))
        .buffer_unordered(segments)
        .try_collect::<Vec<_>>()
        .await
    };

    let chunks = match chunks {
        Ok(chunks) => chunks,
        Err(error) => {
            emit_progress(
                &descriptor,
                downloaded.load(std::sync::atomic::Ordering::Relaxed),
                Some(total_bytes),
                DownloadStatus::Failed(error.to_string()),
            );
            return Err(error);
        }
    };

    pb.finish_and_clear();

    let mut parts = chunks;
    parts.sort_by_key(|(start, _)| *start);

    let mut buffer = Vec::with_capacity(total_len);
    for (_start, mut chunk) in parts {
        buffer.append(&mut chunk);
    }

    anyhow::ensure!(
        buffer.len() == total_len,
        "range assembly mismatch: expected {} bytes, got {}",
        total_len,
        buffer.len()
    );

    emit_progress(
        &descriptor,
        total_bytes,
        Some(total_bytes),
        DownloadStatus::Completed,
    );

    Ok(buffer)
}

fn emit_progress(
    descriptor: &DownloadDescriptor,
    downloaded: u64,
    total: Option<u64>,
    status: DownloadStatus,
) {
    emit(DownloadProgress {
        id: descriptor.id.clone(),
        label: descriptor.label.clone(),
        filename: descriptor.filename.clone(),
        downloaded,
        total,
        status,
    });
}

fn lookup_model_descriptor(repo: &str, filename: &str) -> DownloadDescriptor {
    crate::registry::lookup_model(repo, filename)
        .map(|entry| DownloadDescriptor {
            id: entry.id,
            label: entry.label,
            filename: filename.to_string(),
        })
        .unwrap_or_else(|| DownloadDescriptor {
            id: format!("hf:{repo}:{filename}"),
            label: filename.to_string(),
            filename: filename.to_string(),
        })
}

fn descriptor_from_url(url: &str) -> DownloadDescriptor {
    let filename = url.split('/').next_back().unwrap_or(url).to_string();
    DownloadDescriptor {
        id: format!("url:{url}"),
        label: filename.clone(),
        filename,
    }
}
