use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use futures::{StreamExt, TryStreamExt, stream};
use koharu_types::events::{DownloadProgress, DownloadStatus};
use once_cell::sync::Lazy;
use tokio::sync::broadcast;
use tracing::Instrument;

use crate::hf_hub;
use crate::lock;
use crate::progress::progress_bar;
use crate::range;

const RANGE_CHUNK_SIZE_BYTES: usize = 16 * 1024 * 1024;
const HF_HUB_MAX_ATTEMPTS: usize = 3;
const DOWNLOAD_EVENT_BUFFER: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubAssetSpec {
    pub repo: &'static str,
    pub filename: &'static str,
}

static TX: Lazy<broadcast::Sender<DownloadProgress>> =
    Lazy::new(|| broadcast::channel(DOWNLOAD_EVENT_BUFFER).0);

pub fn subscribe() -> broadcast::Receiver<DownloadProgress> {
    TX.subscribe()
}

pub(crate) fn emit(progress: DownloadProgress) {
    let _ = TX.send(progress);
}

pub fn hub_download_id(repo: &str, filename: &str) -> String {
    format!("hf:{repo}:{filename}")
}

pub fn url_download_id(url: &str) -> String {
    format!("url:{url}")
}

pub async fn model(repo: &str, filename: &str) -> anyhow::Result<PathBuf> {
    let _root_lock = lock::acquire_managed_root(hf_hub::cache().path())?;
    let hf_repo = hf_hub::repo(repo);

    if let Some(path) = hf_hub::cache().repo(hf_repo.clone()).get(filename) {
        return Ok(path);
    }

    let progress_id = hub_download_id(repo, filename);
    for attempt in 1..=HF_HUB_MAX_ATTEMPTS {
        let reporter = hf_hub::Reporter::new(progress_id.clone(), filename);
        let result = hf_hub::api()?
            .repo(hf_repo.clone())
            .download_with_progress(filename, reporter)
            .instrument(tracing::info_span!("hf_download", repo, filename, attempt))
            .await;

        match result {
            Ok(path) => return Ok(path),
            Err(error) => {
                let error = anyhow::Error::new(error).context("failed to download from HF Hub");
                if attempt < HF_HUB_MAX_ATTEMPTS && is_transient_hf_download_error(&error) {
                    let delay = Duration::from_secs(1_u64 << (attempt - 1));
                    tracing::warn!(
                        repo,
                        filename,
                        attempt,
                        max_attempts = HF_HUB_MAX_ATTEMPTS,
                        delay_ms = delay.as_millis() as u64,
                        error = %error,
                        "transient HF download failure, retrying"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                return Err(error);
            }
        }
    }

    unreachable!("HF Hub retry loop must return or error")
}

fn is_transient_hf_download_error(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();

    let non_retryable = [
        "401",
        "403",
        "404",
        "unauthorized",
        "forbidden",
        "not found",
    ];
    if non_retryable
        .iter()
        .any(|pattern| message.contains(pattern))
    {
        return false;
    }

    let retryable = [
        "502",
        "503",
        "504",
        "bad gateway",
        "temporarily unavailable",
        "unexpected eof",
        "handshake",
        "timeout",
        "timed out",
        "connection reset",
        "connection aborted",
        "error sending request",
        "client error (connect)",
        "request error",
    ];

    retryable.iter().any(|pattern| message.contains(pattern))
}

#[tracing::instrument(level = "info")]
pub async fn bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let head = range::head(url)
        .await
        .context(format!("cannot download {url}"))?;
    let total_bytes = head.content_length;

    anyhow::ensure!(total_bytes > 0, "resource reports zero Content-Length");

    let supports_ranges = head.supports_ranges;

    anyhow::ensure!(
        supports_ranges,
        "remote server does not advertise byte ranges"
    );

    let filename = url.split('/').next_back().unwrap_or(url).to_string();
    let progress_id = url_download_id(url);
    let pb = Arc::new(progress_bar(&filename));

    let total_len =
        usize::try_from(total_bytes).context("resource too large to fit into memory")?;
    let chunk_size = total_len.clamp(1, RANGE_CHUNK_SIZE_BYTES);
    let segments = total_len.div_ceil(chunk_size);

    pb.set_length(total_len as u64);

    emit(DownloadProgress {
        id: progress_id.clone(),
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
        let progress_id = progress_id.clone();
        stream::iter((0..segments).map(move |index| {
            let start = (index * chunk_size) as u64;
            let len = ((index + 1) * chunk_size).min(total_len) - (index * chunk_size);
            let end = start + len as u64 - 1;
            let url = Arc::clone(&url);
            let pb = Arc::clone(&pb);
            let downloaded = Arc::clone(&downloaded);
            let filename = filename.clone();
            let progress_id = progress_id.clone();
            async move {
                let chunk = range::get_range(&url, start, end).await?;
                pb.inc(len as u64);
                let current = downloaded
                    .fetch_add(len as u64, std::sync::atomic::Ordering::Relaxed)
                    + len as u64;
                emit(DownloadProgress {
                    id: progress_id,
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
                id: progress_id.clone(),
                filename: filename.clone(),
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
        id: progress_id,
        filename,
        downloaded: total_bytes,
        total: Some(total_bytes),
        status: DownloadStatus::Completed,
    });

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::{hub_download_id, is_transient_hf_download_error, url_download_id};

    #[test]
    fn hub_download_ids_include_repo_and_filename() {
        assert_eq!(
            hub_download_id("owner/repo", "model.gguf"),
            "hf:owner/repo:model.gguf"
        );
    }

    #[test]
    fn url_download_ids_include_full_url() {
        assert_eq!(
            url_download_id("https://example.com/files/model.gguf"),
            "url:https://example.com/files/model.gguf"
        );
    }

    #[test]
    fn classifies_transient_hf_errors() {
        let error = anyhow!("client error (Connect): unexpected EOF during handshake");
        assert!(is_transient_hf_download_error(&error));
    }

    #[test]
    fn does_not_retry_known_non_retryable_hf_errors() {
        let error = anyhow!("HTTP status client error (404 Not Found)");
        assert!(!is_transient_hf_download_error(&error));
    }
}
