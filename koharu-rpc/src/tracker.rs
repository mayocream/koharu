use std::{collections::HashMap, sync::Arc};

use koharu_app::pipeline;
use koharu_core::{
    DownloadProgress, DownloadState, DownloadStatus, JobState, JobStatus, PipelineProgress,
    PipelineStatus, PipelineStep, TransferStatus,
};
use tokio::sync::RwLock;

use crate::shared::SharedState;

/// Tracks active jobs and downloads for polling endpoints.
#[derive(Clone)]
pub struct Tracker {
    inner: Arc<Inner>,
}

struct Inner {
    jobs: RwLock<HashMap<String, JobState>>,
    downloads: RwLock<HashMap<String, DownloadState>>,
}

impl Tracker {
    pub fn new(shared: &SharedState) -> Self {
        let inner = Arc::new(Inner {
            jobs: RwLock::new(HashMap::new()),
            downloads: RwLock::new(HashMap::new()),
        });

        spawn_pipeline_listener(inner.clone());
        spawn_download_listener(inner.clone(), shared.clone());

        Self { inner }
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
        let mut downloads: Vec<_> = self.inner.downloads.read().await.values().cloned().collect();
        downloads.sort_by(|a, b| a.filename.cmp(&b.filename));
        downloads
    }
}

fn spawn_pipeline_listener(inner: Arc<Inner>) {
    let mut rx = pipeline::subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(progress) => {
                    update_job_state(&inner, pipeline_job_state(progress)).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn spawn_download_listener(inner: Arc<Inner>, shared: SharedState) {
    tokio::spawn(async move {
        let mut runtime_rx = shared.subscribe_runtime();
        let mut download_rx = runtime_rx.borrow().clone().subscribe_downloads();

        loop {
            tokio::select! {
                changed = runtime_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    inner.downloads.write().await.clear();
                    download_rx = runtime_rx.borrow().clone().subscribe_downloads();
                }
                received = download_rx.recv() => match received {
                    Ok(progress) => {
                        update_download_state(&inner, download_state(progress)).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        download_rx = runtime_rx.borrow().clone().subscribe_downloads();
                    }
                }
            }
        }
    });
}

async fn update_job_state(inner: &Arc<Inner>, job: JobState) {
    let terminal = !matches!(job.status, JobStatus::Running);
    let mut jobs = inner.jobs.write().await;
    if terminal {
        jobs.remove(&job.id);
    } else {
        jobs.insert(job.id.clone(), job);
    }
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

fn pipeline_job_state(progress: PipelineProgress) -> JobState {
    let (status, error) = match progress.status {
        PipelineStatus::Running => (JobStatus::Running, None),
        PipelineStatus::Completed => (JobStatus::Completed, None),
        PipelineStatus::Cancelled => (JobStatus::Cancelled, None),
        PipelineStatus::Failed(message) => (JobStatus::Failed, Some(message)),
    };

    JobState {
        id: progress.job_id,
        kind: "pipeline".to_string(),
        status,
        step: progress.step.map(pipeline_step_name),
        current_document: progress.current_document,
        total_documents: progress.total_documents,
        current_step_index: progress.current_step_index,
        total_steps: progress.total_steps,
        overall_percent: progress.overall_percent,
        error,
    }
}

fn pipeline_step_name(step: PipelineStep) -> String {
    step.to_string()
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
    use koharu_core::{PipelineStatus, PipelineStep};

    use super::{TransferStatus, download_state, pipeline_job_state};

    #[test]
    fn pipeline_progress_maps_to_job_state() {
        let state = pipeline_job_state(koharu_core::PipelineProgress {
            job_id: "job-1".to_string(),
            status: PipelineStatus::Failed("boom".to_string()),
            step: Some(PipelineStep::Render),
            current_document: 1,
            total_documents: 2,
            current_step_index: 4,
            total_steps: 5,
            overall_percent: 80,
        });

        assert_eq!(state.id, "job-1");
        assert_eq!(state.kind, "pipeline");
        assert_eq!(state.step.as_deref(), Some("render"));
        assert_eq!(state.error.as_deref(), Some("boom"));
    }

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
