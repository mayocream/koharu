use std::{collections::HashMap, sync::Arc};

use koharu_app::pipeline;
use koharu_core::{DownloadProgress, DownloadState, DownloadStatus, JobState, TransferStatus};
use tokio::sync::RwLock;

use crate::shared::SharedState;

/// Tracks active jobs and downloads for polling endpoints.
#[derive(Clone)]
pub struct Tracker {
    inner: Arc<Inner>,
}

struct Inner {
    jobs: pipeline::Jobs,
    downloads: RwLock<HashMap<String, DownloadState>>,
}

impl Tracker {
    pub fn new(shared: &SharedState) -> Self {
        let jobs = Arc::new(RwLock::new(HashMap::new()));
        let inner = Arc::new(Inner {
            jobs: jobs.clone(),
            downloads: RwLock::new(HashMap::new()),
        });

        spawn_download_listener(inner.clone(), shared.clone());

        Self { inner }
    }

    pub fn jobs(&self) -> pipeline::Jobs {
        self.inner.jobs.clone()
    }

    pub async fn list_jobs(&self) -> Vec<JobState> {
        let mut jobs: Vec<_> = self.inner.jobs.read().await.values().cloned().collect();
        jobs.sort_by(|a, b| a.id.cmp(&b.id));
        jobs
    }

    pub async fn get_job(&self, job_id: &str) -> Option<JobState> {
        self.inner.jobs.read().await.get(job_id).cloned()
    }

    pub async fn publish_job(&self, job: JobState) {
        update_job_state(&self.inner, job).await;
    }

    pub async fn list_downloads(&self) -> Vec<DownloadState> {
        let mut downloads: Vec<_> = self
            .inner
            .downloads
            .read()
            .await
            .values()
            .cloned()
            .collect();
        downloads.sort_by(|a, b| a.filename.cmp(&b.filename));
        downloads
    }
}

fn spawn_download_listener(inner: Arc<Inner>, shared: SharedState) {
    tokio::spawn(async move {
        let mut download_rx = shared.runtime().subscribe_downloads();

        loop {
            match download_rx.recv().await {
                Ok(progress) => {
                    update_download_state(&inner, download_state(progress)).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn update_job_state(inner: &Arc<Inner>, job: JobState) {
    inner.jobs.write().await.insert(job.id.clone(), job);
}

async fn update_download_state(inner: &Arc<Inner>, download: DownloadState) {
    let terminal = !matches!(
        download.status,
        TransferStatus::Started | TransferStatus::Downloading
    );
    let mut downloads = inner.downloads.write().await;
    if terminal {
        downloads.remove(&download.id);
    } else {
        downloads.insert(download.id.clone(), download);
    }
}

fn download_state(progress: DownloadProgress) -> DownloadState {
    let (status, error) = match progress.status {
        DownloadStatus::Started => (TransferStatus::Started, None),
        DownloadStatus::Downloading => (TransferStatus::Downloading, None),
        DownloadStatus::Completed => (TransferStatus::Completed, None),
        DownloadStatus::Failed(message) => (TransferStatus::Failed, Some(message)),
    };

    DownloadState {
        id: progress.filename.clone(),
        filename: progress.filename,
        downloaded: progress.downloaded,
        total: progress.total,
        status,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::{TransferStatus, download_state};

    #[test]
    fn download_progress_maps_to_download_state() {
        let state = download_state(koharu_core::DownloadProgress {
            filename: "model.bin".to_string(),
            downloaded: 32,
            total: Some(64),
            status: koharu_core::DownloadStatus::Completed,
        });

        assert_eq!(state.id, "model.bin");
        assert!(matches!(state.status, TransferStatus::Completed));
        assert_eq!(state.total, Some(64));
    }
}
