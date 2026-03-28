use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use hf_hub::{
    Cache, Repo,
    api::tokio::{Api, ApiBuilder, Progress},
};
use indicatif::ProgressBar;
use koharu_types::events::{DownloadProgress, DownloadStatus};
use once_cell::sync::OnceCell;

use crate::config::download_config;
use crate::download::emit;
use crate::paths;
use crate::progress::progress_bar;

static CACHE_DIR: OnceCell<PathBuf> = OnceCell::new();
static HF_PROXY_ENV_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

fn get_cache_dir() -> &'static PathBuf {
    CACHE_DIR.get_or_init(paths::model_root)
}

pub fn set_cache_dir(path: PathBuf) -> anyhow::Result<()> {
    CACHE_DIR
        .set(path)
        .map_err(|_| anyhow::anyhow!("cache dir has already been set"))
}

pub fn api() -> anyhow::Result<Api> {
    let config = download_config();
    let _env_lock = HF_PROXY_ENV_LOCK
        .lock()
        .expect("hf proxy env lock poisoned");
    let _proxy_env = ScopedProxyEnv::apply(config.proxy_url.as_deref());

    tracing::info!(
        cache_dir = %get_cache_dir().display(),
        proxy_configured = config.proxy_url.is_some(),
        "building HF API client"
    );

    ApiBuilder::new()
        .with_cache_dir(get_cache_dir().to_path_buf())
        .high()
        .build()
        .map_err(|error| anyhow::anyhow!("build HF API client: {error}"))
}

pub fn cache() -> Cache {
    Cache::new(get_cache_dir().to_path_buf())
}

pub fn repo(name: &str) -> Repo {
    Repo::model(name.to_string())
}

pub fn cached_model_path(repo_name: &str, filename: &str) -> Option<PathBuf> {
    cache().repo(repo(repo_name)).get(filename)
}

pub fn repo_cache_dir(repo_name: &str) -> PathBuf {
    let cache = cache();
    cache.path().join(repo(repo_name).folder_name())
}

struct ScopedProxyEnv {
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl ScopedProxyEnv {
    fn apply(proxy_url: Option<&str>) -> Self {
        let keys = [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ];
        let mut previous = Vec::with_capacity(keys.len());
        for key in keys {
            previous.push((key, std::env::var_os(key)));
            if let Some(proxy_url) = proxy_url {
                unsafe {
                    std::env::set_var(key, proxy_url);
                }
            } else {
                unsafe {
                    std::env::remove_var(key);
                }
            }
        }
        Self { previous }
    }
}

impl Drop for ScopedProxyEnv {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..) {
            if let Some(value) = value {
                unsafe {
                    std::env::set_var(key, value);
                }
            } else {
                unsafe {
                    std::env::remove_var(key);
                }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct Reporter {
    pb: ProgressBar,
    id: String,
    filename: String,
    downloaded: Arc<AtomicU64>,
    total: u64,
}

impl Reporter {
    pub fn new(id: String, filename: &str) -> Self {
        Self {
            pb: progress_bar(filename),
            id,
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
            id: self.id.clone(),
            filename: self.filename.clone(),
            downloaded: 0,
            total: Some(self.total),
            status: DownloadStatus::Started,
        });
    }

    async fn update(&mut self, size: usize) {
        let current = self.downloaded.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
        self.pb.inc(size as u64);
        emit(DownloadProgress {
            id: self.id.clone(),
            filename: self.filename.clone(),
            downloaded: current,
            total: Some(self.total),
            status: DownloadStatus::Downloading,
        });
    }

    async fn finish(&mut self) {
        self.pb.finish_and_clear();
        emit(DownloadProgress {
            id: self.id.clone(),
            filename: self.filename.clone(),
            downloaded: self.downloaded.load(Ordering::Relaxed),
            total: Some(self.total),
            status: DownloadStatus::Completed,
        });
    }
}
