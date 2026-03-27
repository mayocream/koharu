use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use futures::{StreamExt, TryStreamExt, stream};
use koharu_types::events::{DownloadProgress, DownloadStatus};
use once_cell::sync::Lazy;
use tokio::sync::broadcast;
use tracing::Instrument;

use crate::hf_hub;
use crate::progress::progress_bar;
use crate::range;

const RANGE_CHUNK_SIZE_BYTES: usize = 16 * 1024 * 1024;

static TX: Lazy<broadcast::Sender<DownloadProgress>> = Lazy::new(|| broadcast::channel(256).0);

pub fn subscribe() -> broadcast::Receiver<DownloadProgress> {
    TX.subscribe()
}

pub(crate) fn emit(progress: DownloadProgress) {
    let _ = TX.send(progress);
}

pub async fn model(repo: &str, filename: &str) -> anyhow::Result<PathBuf> {
    let hf_repo = hf_hub::repo(repo);

    if let Some(path) = hf_hub::cache().repo(hf_repo.clone()).get(filename) {
        return Ok(path);
    }

    let reporter = hf_hub::Reporter::new(filename);

    let path = hf_hub::api()
        .repo(hf_repo)
        .download_with_progress(filename, reporter)
        .instrument(tracing::info_span!("hf_download", repo, filename))
        .await
        .context("failed to download from HF Hub")?;

    Ok(path)
}

#[tracing::instrument(level = "info")]
pub async fn bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let head = range::head(url).await.ok();
    let total_bytes = head.and_then(|h| h.content_length);
    let supports_ranges = head.map(|h| h.supports_ranges).unwrap_or(false);

    if supports_ranges && total_bytes.is_some_and(|len| len > 0) {
        let total_bytes = total_bytes.unwrap();
        match bytes_via_range(url, total_bytes).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                tracing::warn!(%url, %e, "range download failed, falling back to stream");
            }
        }
    }

    bytes_via_stream(url, total_bytes).await
}

async fn bytes_via_range(url: &str, total_bytes: u64) -> anyhow::Result<Vec<u8>> {
    let filename = url.split('/').next_back().unwrap_or(url).to_string();
    let pb = Arc::new(progress_bar(&filename));

    let total_len =
        usize::try_from(total_bytes).context("resource too large to fit into memory")?;
    let chunk_size = total_len.clamp(1, RANGE_CHUNK_SIZE_BYTES);
    let segments = total_len.div_ceil(chunk_size);

    pb.set_length(total_len as u64);

    emit(DownloadProgress {
        filename: filename.clone(),
        downloaded: 0,
        total: Some(total_bytes),
        status: DownloadStatus::Started,
    });

    tracing::debug!(
        %url,
        total_bytes,
        segments,
        "downloading resource via HTTP range requests"
    );

    let downloaded = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let chunks = {
        let url = Arc::new(url.to_string());
        let pb = Arc::clone(&pb);
        let downloaded = Arc::clone(&downloaded);
        let filename = filename.clone();
        stream::iter((0..segments).map(move |index| {
            let start = (index * chunk_size) as u64;
            let len = ((index + 1) * chunk_size).min(total_len) - (index * chunk_size);
            let end = start + len as u64 - 1;
            let url = Arc::clone(&url);
            let pb = Arc::clone(&pb);
            let downloaded = Arc::clone(&downloaded);
            let filename = filename.clone();
            async move {
                let chunk = range::get_range(&url, start, end).await?;
                pb.inc(len as u64);
                let current = downloaded
                    .fetch_add(len as u64, std::sync::atomic::Ordering::Relaxed)
                    + len as u64;
                emit(DownloadProgress {
                    filename,
                    downloaded: current,
                    total: Some(total_bytes),
                    status: DownloadStatus::Downloading,
                });
                Ok::<_, anyhow::Error>((start, chunk))
            }
        }))
        .buffer_unordered(segments)
        .try_collect::<Vec<_>>()
        .await
    };

    let chunks = match chunks {
        Ok(c) => c,
        Err(e) => {
            emit(DownloadProgress {
                filename,
                downloaded: downloaded.load(std::sync::atomic::Ordering::Relaxed),
                total: Some(total_bytes),
                status: DownloadStatus::Failed(e.to_string()),
            });
            return Err(e);
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

    emit(DownloadProgress {
        filename,
        downloaded: total_bytes,
        total: Some(total_bytes),
        status: DownloadStatus::Completed,
    });

    Ok(buffer)
}

async fn bytes_via_stream(url: &str, total_bytes: Option<u64>) -> anyhow::Result<Vec<u8>> {
    let filename = url.split('/').next_back().unwrap_or(url).to_string();
    let pb = Arc::new(progress_bar(&filename));
    if let Some(total) = total_bytes {
        pb.set_length(total);
    }

    emit(DownloadProgress {
        filename: filename.clone(),
        downloaded: 0,
        total: total_bytes,
        status: DownloadStatus::Started,
    });

    tracing::debug!(%url, ?total_bytes, "downloading resource via HTTP stream");

    let response = crate::http::http_client()
        .get(url)
        .send()
        .await?
        .error_for_status()?;

    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    if let Some(total) = total_bytes {
        buffer.reserve(total as usize);
    }

    let mut downloaded = 0u64;
    while let Some(chunk) = stream.try_next().await? {
        downloaded += chunk.len() as u64;
        buffer.extend_from_slice(&chunk);
        pb.inc(chunk.len() as u64);
        emit(DownloadProgress {
            filename: filename.clone(),
            downloaded,
            total: total_bytes,
            status: DownloadStatus::Downloading,
        });
    }

    pb.finish_and_clear();

    emit(DownloadProgress {
        filename,
        downloaded,
        total: Some(downloaded),
        status: DownloadStatus::Completed,
    });

    Ok(buffer)
}
