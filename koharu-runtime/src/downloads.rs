use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use hf_hub::{
    Cache,
    api::tokio::{ApiBuilder, Progress},
};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use koharu_core::events::{DownloadProgress, DownloadStatus};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::broadcast;

use crate::runtime::RuntimeHttpConfig;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

// ---------------------------------------------------------------------------
// Downloads — unified download manager
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Downloads {
    downloads_root: PathBuf,
    huggingface_cache: Cache,
    base_client: reqwest::Client,
    client: Arc<ClientWithMiddleware>,
    tx: broadcast::Sender<DownloadProgress>,
    progress: Arc<MultiProgress>,
}

impl Downloads {
    pub(crate) fn new(
        downloads_root: PathBuf,
        huggingface_root: PathBuf,
        http: &RuntimeHttpConfig,
    ) -> Result<Self> {
        let base = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(http.connect_timeout_secs))
            .read_timeout(Duration::from_secs(http.read_timeout_secs))
            .build()?;
        let client = Arc::new(
            ClientBuilder::new(base.clone())
                .with(RetryTransientMiddleware::new_with_policy(
                    ExponentialBackoff::builder().build_with_max_retries(http.max_retries),
                ))
                .build(),
        );

        Ok(Self {
            downloads_root,
            huggingface_cache: Cache::new(huggingface_root),
            base_client: base,
            client,
            tx: broadcast::channel(256).0,
            progress: Arc::new(MultiProgress::new()),
        })
    }

    pub fn client(&self) -> Arc<ClientWithMiddleware> {
        Arc::clone(&self.client)
    }

    pub fn base_client(&self) -> reqwest::Client {
        self.base_client.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DownloadProgress> {
        self.tx.subscribe()
    }

    /// Download a HuggingFace model file, using the local cache first.
    pub async fn huggingface_model(&self, repo: &str, filename: &str) -> Result<PathBuf> {
        if let Some(path) = self.huggingface_cache.model(repo.to_string()).get(filename) {
            return Ok(path);
        }

        let api = ApiBuilder::from_cache(self.huggingface_cache.clone())
            .with_progress(false)
            .high() // high concurrency
            .build()
            .context("failed to build HF Hub API")?;
        let progress = HfProgress::new(self.tx.clone(), &self.progress, filename);

        match api
            .model(repo.to_string())
            .download_with_progress(filename, progress.clone())
            .await
        {
            Ok(path) => Ok(path),
            Err(error) => {
                let error = anyhow::Error::new(error).context(format!(
                    "failed to download HF model file `{repo}/{filename}`"
                ));
                progress.fail(&error).await;
                Err(error)
            }
        }
    }

    /// Download a file to the downloads cache, returning the cached path.
    pub(crate) async fn cached_download(&self, url: &str, file_name: &str) -> Result<PathBuf> {
        let destination = self.downloads_root.join(file_name);
        if destination.exists() {
            return Ok(destination);
        }
        self.download_to(url, &destination, file_name).await?;
        Ok(destination)
    }

    async fn download_to(&self, url: &str, destination: &Path, label: &str) -> Result<()> {
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }

        let temp = part_path(destination)?;
        tokio::fs::remove_file(&temp).await.ok();

        let response = self
            .client
            .get(url)
            .send()
            .await
            .with_context(|| format!("failed to start download `{url}`"))?
            .error_for_status()
            .with_context(|| format!("download failed for `{url}`"))?;

        let reporter = self.begin(label);
        reporter.start(response.content_length());

        let write_result = async {
            let file = tokio::fs::File::create(&temp)
                .await
                .with_context(|| format!("failed to create `{}`", temp.display()))?;
            let mut writer = BufWriter::new(file);
            let mut stream = response.bytes_stream();

            while let Some(chunk) = stream.next().await {
                let chunk = chunk.with_context(|| format!("download interrupted for `{url}`"))?;
                writer
                    .write_all(&chunk)
                    .await
                    .with_context(|| format!("failed to write `{}`", temp.display()))?;
                reporter.advance(chunk.len());
            }

            writer
                .flush()
                .await
                .with_context(|| format!("failed to flush `{}`", temp.display()))?;
            Ok::<_, anyhow::Error>(())
        }
        .await;

        if let Err(err) = write_result {
            reporter.fail(&err);
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
        reporter.finish();
        Ok(())
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
        self.emit(DownloadStatus::Failed(error.to_string()));
    }

    fn emit(&self, status: DownloadStatus) {
        let total = self.total.load(Ordering::Relaxed);
        let _ = self.tx.send(DownloadProgress {
            filename: self.filename.to_string(),
            downloaded: self.downloaded.load(Ordering::Relaxed),
            total: (total != UNKNOWN_TOTAL).then_some(total),
            status,
        });
    }
}

// ---------------------------------------------------------------------------
// HuggingFace progress adapter
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct HfProgress {
    reporter: TransferReporter,
}

impl HfProgress {
    fn new(tx: broadcast::Sender<DownloadProgress>, multi: &MultiProgress, label: &str) -> Self {
        let bar = multi.add(ProgressBar::new_spinner());
        bar.enable_steady_tick(Duration::from_millis(120));
        bar.set_style(
            ProgressStyle::with_template(
                "{msg} [{elapsed_precise}] [{wide_bar}] {bytes}/{total_bytes} ({eta})",
            )
            .expect("progress style"),
        );
        bar.set_message(label.to_string());
        Self {
            reporter: TransferReporter::new(tx, bar, label),
        }
    }

    async fn fail(&self, error: &anyhow::Error) {
        self.reporter.fail(error);
    }
}

impl Progress for HfProgress {
    async fn init(&mut self, size: usize, _filename: &str) {
        self.reporter.start(Some(size as u64));
    }

    async fn update(&mut self, size: usize) {
        self.reporter.advance(size);
    }

    async fn finish(&mut self) {
        self.reporter.finish();
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
