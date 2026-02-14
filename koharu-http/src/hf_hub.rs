use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use hf_hub::{
    Cache, Repo,
    api::tokio::{Api, ApiBuilder, Progress},
};
use indicatif::ProgressBar;
use once_cell::sync::{Lazy, OnceCell};

use crate::download::{DownloadProgress, Status, emit};
use crate::progress::progress_bar;

static CACHE_DIR: OnceCell<PathBuf> = OnceCell::new();

static HF_API: Lazy<Api> = Lazy::new(|| {
    ApiBuilder::new()
        .with_cache_dir(get_cache_dir().to_path_buf())
        .high()
        .build()
        .expect("build HF API client")
});
static HF_CACHE: Lazy<Cache> = Lazy::new(|| Cache::new(get_cache_dir().to_path_buf()));

fn get_cache_dir() -> &'static PathBuf {
    CACHE_DIR.get_or_init(|| {
        dirs::cache_dir()
            .unwrap_or_default()
            .join("Koharu")
            .join("models")
    })
}

pub fn set_cache_dir(path: PathBuf) -> anyhow::Result<()> {
    CACHE_DIR
        .set(path)
        .map_err(|_| anyhow::anyhow!("cache dir has already been set"))
}

pub fn api() -> &'static Api {
    &HF_API
}

pub fn cache() -> &'static Cache {
    &HF_CACHE
}

pub fn repo(name: &str) -> Repo {
    Repo::model(name.to_string())
}

#[derive(Clone)]
pub(crate) struct Reporter {
    pb: ProgressBar,
    filename: String,
    downloaded: Arc<AtomicU64>,
    total: u64,
}

impl Reporter {
    pub fn new(filename: &str) -> Self {
        Self {
            pb: progress_bar(filename),
            filename: filename.to_string(),
            downloaded: Arc::new(AtomicU64::new(0)),
            total: 0,
        }
    }
}

impl Progress for Reporter {
    async fn init(&mut self, size: usize, filename: &str) {
        self.filename = filename.to_string();
        self.downloaded.store(0, Ordering::Relaxed);
        self.total = size as u64;
        self.pb.set_length(size as u64);
        self.pb.set_position(0);
        emit(DownloadProgress {
            filename: self.filename.clone(),
            downloaded: 0,
            total: Some(self.total),
            status: Status::Started,
        });
    }

    async fn update(&mut self, size: usize) {
        let current = self.downloaded.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
        self.pb.inc(size as u64);
        emit(DownloadProgress {
            filename: self.filename.clone(),
            downloaded: current,
            total: Some(self.total),
            status: Status::Downloading,
        });
    }

    async fn finish(&mut self) {
        self.pb.finish_and_clear();
        emit(DownloadProgress {
            filename: self.filename.clone(),
            downloaded: self.downloaded.load(Ordering::Relaxed),
            total: Some(self.total),
            status: Status::Completed,
        });
    }
}
