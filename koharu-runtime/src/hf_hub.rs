use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use hf_hub::{
    Cache, Repo,
    api::tokio::{Api, ApiBuilder, Progress},
};
use indicatif::ProgressBar;
use koharu_core::{DownloadProgress, DownloadStatus};

use crate::download::{DownloadDescriptor, emit};
use crate::progress::{DownloadEventThrottle, progress_bar};

pub fn api(cache_dir: &Path) -> Api {
    ApiBuilder::new()
        .with_cache_dir(cache_dir.to_path_buf())
        .high()
        .build()
        .expect("build HF API client")
}

pub fn cache(cache_dir: &Path) -> Cache {
    Cache::new(cache_dir.to_path_buf())
}

pub fn repo(name: &str) -> Repo {
    Repo::model(name.to_string())
}

#[derive(Clone)]
pub(crate) struct Reporter {
    pb: ProgressBar,
    descriptor: DownloadDescriptor,
    downloaded: Arc<AtomicU64>,
    total: u64,
    throttle: DownloadEventThrottle,
}

impl Reporter {
    pub fn new(descriptor: DownloadDescriptor) -> Self {
        Self {
            pb: progress_bar(&descriptor.filename),
            descriptor,
            downloaded: Arc::new(AtomicU64::new(0)),
            total: 0,
            throttle: DownloadEventThrottle::default(),
        }
    }
}

impl Progress for Reporter {
    async fn init(&mut self, size: usize, filename: &str) {
        self.descriptor.filename = filename.to_string();
        self.downloaded.store(0, Ordering::Relaxed);
        self.total = size as u64;
        self.pb.set_length(size as u64);
        self.pb.set_position(0);
        emit_progress(
            &self.descriptor,
            0,
            Some(self.total),
            DownloadStatus::Started,
        );
        self.throttle.mark_emitted(0, Some(self.total));
    }

    async fn update(&mut self, size: usize) {
        let current = self.downloaded.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
        self.pb.inc(size as u64);
        if self.throttle.should_emit(current, Some(self.total)) {
            emit_progress(
                &self.descriptor,
                current,
                Some(self.total),
                DownloadStatus::Downloading,
            );
        }
    }

    async fn finish(&mut self) {
        self.pb.finish_and_clear();
        emit_progress(
            &self.descriptor,
            self.downloaded.load(Ordering::Relaxed),
            Some(self.total),
            DownloadStatus::Completed,
        );
    }
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
