use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt, TryStreamExt};
use hf_hub::{
    Cache, Repo, RepoType,
    api::tokio::{ApiBuilder, Metadata},
};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use koharu_core::events::{DownloadProgress, DownloadStatus};
use reqwest::header::{CONTENT_LENGTH, RANGE};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::broadcast;

use crate::runtime::{RuntimeHttpClient, RuntimeHttpConfig};

/// 10 MiB per ranged GET — same size hf-hub's `.high()` mode uses. Short enough
/// that reqwest's read_timeout catches a stalled connection quickly, and the
/// retry middleware can restart the chunk.
const CHUNK_SIZE: u64 = 10 * 1024 * 1024;

/// hf-hub's internal client has no read timeout, so we cap the metadata call
/// ourselves. The response body is a single byte — a short cap is safe.
const HF_METADATA_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Downloads — unified download manager
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Downloads {
    downloads_root: PathBuf,
    huggingface_cache: Cache,
    client: RuntimeHttpClient,
    tx: broadcast::Sender<DownloadProgress>,
    progress: Arc<MultiProgress>,
}

impl Downloads {
    pub(crate) fn new(
        downloads_root: PathBuf,
        huggingface_root: PathBuf,
        http: &RuntimeHttpConfig,
    ) -> Result<Self> {
        let client = http.build_client()?;

        Ok(Self {
            downloads_root,
            huggingface_cache: Cache::new(huggingface_root),
            client,
            tx: broadcast::channel(256).0,
            progress: Arc::new(MultiProgress::new()),
        })
    }

    pub fn client(&self) -> RuntimeHttpClient {
        Arc::clone(&self.client)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DownloadProgress> {
        self.tx.subscribe()
    }

    /// Download a HuggingFace model file, using the local cache first.
    ///
    /// hf-hub resolves URL + metadata + cache layout; the byte transfer runs
    /// on our retry-configured client so a stalled chunk is retried by the
    /// middleware instead of hanging the future.
    pub async fn huggingface_model(&self, repo: &str, filename: &str) -> Result<PathBuf> {
        let cache_repo = self
            .huggingface_cache
            .repo(Repo::new(repo.to_string(), RepoType::Model));

        if let Some(path) = cache_repo.get(filename) {
            return Ok(path);
        }

        let api = ApiBuilder::from_cache(self.huggingface_cache.clone())
            .with_progress(false)
            .with_user_agent("koharu", env!("CARGO_PKG_VERSION"))
            .build()
            .context("failed to build HF Hub API")?;
        let repo_handle = api.model(repo.to_string());
        let url = repo_handle.url(filename);

        let metadata: Metadata = tokio::time::timeout(HF_METADATA_TIMEOUT, api.metadata(&url))
            .await
            .map_err(|_| anyhow::anyhow!("HF metadata request timed out for `{repo}/{filename}`"))?
            .with_context(|| format!("failed to fetch HF metadata for `{repo}/{filename}`"))?;

        let blob_path = cache_repo.blob_path(metadata.etag());
        if let Some(parent) = blob_path.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("failed to create HF blob directory `{}`", parent.display())
            })?;
        }

        if !blob_path.exists() {
            let reporter = self.begin(filename);
            if let Err(error) = self
                .ranged_download(&url, &blob_path, &reporter, Some(metadata.size() as u64))
                .await
            {
                reporter.fail(&error);
                return Err(error.context(format!(
                    "failed to download HF model file `{repo}/{filename}`"
                )));
            }
            reporter.finish();
        }

        let pointer_dir = cache_repo.pointer_path(metadata.commit_hash());
        let pointer_path = pointer_dir.join(filename);
        if let Some(parent) = pointer_path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        if !pointer_path.exists() {
            #[cfg(target_os = "windows")]
            std::os::windows::fs::symlink_file(&blob_path, &pointer_path).ok();
            #[cfg(target_family = "unix")]
            std::os::unix::fs::symlink(&blob_path, &pointer_path).ok();
        }
        cache_repo
            .create_ref(metadata.commit_hash())
            .context("failed to create HF cache ref")?;

        Ok(if pointer_path.exists() {
            pointer_path
        } else {
            blob_path
        })
    }

    /// Download a file to the downloads cache, returning the cached path.
    pub(crate) async fn cached_download(&self, url: &str, file_name: &str) -> Result<PathBuf> {
        let destination = self.downloads_root.join(file_name);
        if destination.exists() {
            return Ok(destination);
        }

        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }

        let reporter = self.begin(file_name);
        if let Err(error) = self
            .ranged_download(url, &destination, &reporter, None)
            .await
        {
            reporter.fail(&error);
            return Err(error);
        }
        reporter.finish();
        Ok(destination)
    }

    /// Stream a URL to `destination` as a set of ranged GETs running up to
    /// `chunk_parallelism()` in flight (defaults to the host's CPU core count).
    /// The temp file is pre-allocated to the full size so each worker can
    /// seek-and-write its range independently. Transient failures surface as
    /// `Err`; the retry middleware on `self.client` retries at the request
    /// level, and when retries are exhausted the whole download fails cleanly.
    async fn ranged_download(
        &self,
        url: &str,
        destination: &Path,
        reporter: &TransferReporter,
        total_hint: Option<u64>,
    ) -> Result<()> {
        let total = match total_hint {
            Some(t) => t,
            None => self.probe_content_length(url).await?,
        };
        reporter.start(Some(total));

        let temp = part_path(destination)?;
        tokio::fs::remove_file(&temp).await.ok();
        {
            let file = tokio::fs::File::create(&temp)
                .await
                .with_context(|| format!("failed to create `{}`", temp.display()))?;
            file.set_len(total)
                .await
                .with_context(|| format!("failed to preallocate `{}`", temp.display()))?;
        }

        let mut chunks = Vec::new();
        let mut start: u64 = 0;
        while start < total {
            let stop = (start + CHUNK_SIZE).min(total) - 1;
            chunks.push((start, stop));
            start = stop + 1;
        }

        let temp_ref: &Path = &temp;
        let write_result: Result<()> = stream::iter(chunks)
            .map(|(start, stop)| async move {
                let range = format!("bytes={start}-{stop}");
                let response = self
                    .client
                    .get(url)
                    .header(RANGE, &range)
                    .send()
                    .await
                    .with_context(|| format!("failed to fetch range {range} of `{url}`"))?
                    .error_for_status()
                    .with_context(|| format!("fetch failed for range {range} of `{url}`"))?;
                let bytes = response
                    .bytes()
                    .await
                    .with_context(|| format!("failed to read range {range} of `{url}`"))?;
                let mut file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .open(temp_ref)
                    .await
                    .with_context(|| format!("failed to open `{}`", temp_ref.display()))?;
                file.seek(std::io::SeekFrom::Start(start))
                    .await
                    .with_context(|| format!("failed to seek in `{}`", temp_ref.display()))?;
                file.write_all(&bytes)
                    .await
                    .with_context(|| format!("failed to write `{}`", temp_ref.display()))?;
                file.flush()
                    .await
                    .with_context(|| format!("failed to flush `{}`", temp_ref.display()))?;
                reporter.advance(bytes.len());
                Ok::<_, anyhow::Error>(())
            })
            .buffer_unordered(num_cpus::get())
            .try_collect()
            .await;

        if let Err(err) = write_result {
            tokio::fs::remove_file(&temp).await.ok();
            return Err(err);
        }

        tokio::fs::remove_file(destination).await.ok();
        tokio::fs::rename(&temp, destination)
            .await
            .with_context(|| {
                format!(
                    "failed to rename `{}` → `{}`",
                    temp.display(),
                    destination.display()
                )
            })?;
        Ok(())
    }

    async fn probe_content_length(&self, url: &str) -> Result<u64> {
        let response = self
            .client
            .head(url)
            .send()
            .await
            .with_context(|| format!("failed to HEAD `{url}`"))?
            .error_for_status()
            .with_context(|| format!("HEAD failed for `{url}`"))?;

        let content_length = response
            .headers()
            .get(CONTENT_LENGTH)
            .ok_or_else(|| anyhow::anyhow!("missing Content-Length for `{url}`"))?
            .to_str()
            .context("invalid Content-Length header")?;
        content_length
            .trim()
            .parse::<u64>()
            .with_context(|| format!("invalid Content-Length `{content_length}` for `{url}`"))
    }

    fn begin(&self, label: &str) -> TransferReporter {
        let bar = self.progress.add(ProgressBar::new_spinner());
        bar.enable_steady_tick(Duration::from_millis(120));
        bar.set_style(
            ProgressStyle::with_template(
                "{msg} [{elapsed_precise}] [{wide_bar}] {bytes}/{total_bytes} ({eta})",
            )
            .expect("progress style"),
        );
        bar.set_message(label.to_string());
        TransferReporter::new(self.tx.clone(), bar, label)
    }
}

// ---------------------------------------------------------------------------
// Transfer progress reporter
// ---------------------------------------------------------------------------

const UNKNOWN_TOTAL: u64 = u64::MAX;

#[derive(Clone)]
struct TransferReporter {
    tx: broadcast::Sender<DownloadProgress>,
    bar: ProgressBar,
    filename: Arc<str>,
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
}

impl TransferReporter {
    fn new(tx: broadcast::Sender<DownloadProgress>, bar: ProgressBar, label: &str) -> Self {
        Self {
            tx,
            bar,
            filename: Arc::<str>::from(label),
            downloaded: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(UNKNOWN_TOTAL)),
        }
    }

    fn start(&self, total: Option<u64>) {
        self.total
            .store(total.unwrap_or(UNKNOWN_TOTAL), Ordering::Relaxed);
        self.downloaded.store(0, Ordering::Relaxed);
        self.bar.set_length(total.unwrap_or(0));
        self.bar.set_position(0);
        self.emit(DownloadStatus::Started);
    }

    fn advance(&self, delta: usize) {
        self.downloaded.fetch_add(delta as u64, Ordering::Relaxed);
        self.bar.inc(delta as u64);
        self.emit(DownloadStatus::Downloading);
    }

    fn finish(&self) {
        self.bar.finish_and_clear();
        self.emit(DownloadStatus::Completed);
    }

    fn fail(&self, error: &anyhow::Error) {
        self.bar.finish_and_clear();
        self.emit(DownloadStatus::Failed {
            reason: error.to_string(),
        });
    }

    fn emit(&self, status: DownloadStatus) {
        let total = self.total.load(Ordering::Relaxed);
        let _ = self.tx.send(DownloadProgress {
            id: self.filename.to_string(),
            filename: self.filename.to_string(),
            downloaded: self.downloaded.load(Ordering::Relaxed),
            total: (total != UNKNOWN_TOTAL).then_some(total),
            status,
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn part_path(destination: &Path) -> Result<PathBuf> {
    let file_name = destination.file_name().ok_or_else(|| {
        anyhow::anyhow!(
            "destination `{}` does not have a filename",
            destination.display()
        )
    })?;
    Ok(destination.with_file_name(format!("{}.part", file_name.to_string_lossy())))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::part_path;

    #[test]
    fn partial_download_path_appends_suffix() {
        let part = part_path(Path::new("/tmp/models/config.json")).unwrap();
        assert_eq!(part, Path::new("/tmp/models/config.json.part"));
    }
}
