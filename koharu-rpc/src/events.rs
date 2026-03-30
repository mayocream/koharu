use std::{collections::HashMap, sync::Arc, time::Duration};

use koharu_app::{pipeline, state_tx};
use koharu_core::{
    DocumentChangedEvent, DocumentSummary, DocumentsChangedEvent, DownloadState, DownloadStatus,
    JobState, JobStatus, LlmState, PipelineProgress, PipelineStatus, PipelineStep, SnapshotEvent,
    TransferStatus,
};
use koharu_runtime::download;
use tokio::sync::{RwLock, broadcast};

use crate::shared::{SharedResources, get_resources};

#[derive(Debug, Clone)]
pub enum ApiEvent {
    DocumentsChanged(DocumentsChangedEvent),
    DocumentChanged(DocumentChangedEvent),
    JobChanged(JobState),
    DownloadChanged(DownloadState),
    LlmChanged(LlmState),
}

#[derive(Clone)]
pub struct EventHub {
    inner: Arc<Inner>,
}

struct Inner {
    shared: SharedResources,
    tx: broadcast::Sender<ApiEvent>,
    jobs: RwLock<HashMap<String, JobState>>,
    downloads: RwLock<HashMap<String, DownloadState>>,
}

impl EventHub {
    pub fn new(shared: SharedResources) -> Self {
        let inner = Arc::new(Inner {
            shared,
            tx: broadcast::channel(256).0,
            jobs: RwLock::new(HashMap::new()),
            downloads: RwLock::new(HashMap::new()),
        });

        spawn_state_listener(inner.clone());
        spawn_pipeline_listener(inner.clone());
        spawn_download_listener(inner.clone());
        spawn_llm_listener(inner.clone());

        Self { inner }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApiEvent> {
        self.inner.tx.subscribe()
    }

    pub async fn snapshot(&self) -> anyhow::Result<SnapshotEvent> {
        let resources = get_resources(&self.inner.shared)?;
        let guard = resources.state.read().await;
        let documents = guard.documents.iter().map(DocumentSummary::from).collect();
        drop(guard);
        let llm = resources.llm.snapshot().await;

        let mut jobs = self
            .inner
            .jobs
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        jobs.sort_by(|left, right| left.id.cmp(&right.id));

        let mut downloads = self
            .inner
            .downloads
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        downloads.sort_by(|left, right| left.filename.cmp(&right.filename));

        Ok(SnapshotEvent {
            documents,
            llm,
            jobs,
            downloads,
        })
    }

    pub async fn publish_job(&self, job: JobState) {
        update_job_state(&self.inner, job).await;
    }
}

fn spawn_state_listener(inner: Arc<Inner>) {
    let mut rx = state_tx::subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(state_tx::StateEvent::DocumentsChanged) => {
                    let Ok(resources) = get_resources(&inner.shared) else {
                        continue;
                    };
                    let guard = resources.state.read().await;
                    let documents = guard.documents.iter().map(DocumentSummary::from).collect();
                    drop(guard);
                    emit(
                        &inner,
                        ApiEvent::DocumentsChanged(DocumentsChangedEvent { documents }),
                    );
                }
                Ok(state_tx::StateEvent::DocumentChanged {
                    document_id,
                    revision,
                    changed,
                }) => {
                    emit(
                        &inner,
                        ApiEvent::DocumentChanged(DocumentChangedEvent {
                            document_id,
                            revision,
                            changed,
                        }),
                    );
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        "State event listener lagged by {n} events, re-emitting documents"
                    );
                    let Ok(resources) = get_resources(&inner.shared) else {
                        continue;
                    };
                    let guard = resources.state.read().await;
                    let documents = guard.documents.iter().map(DocumentSummary::from).collect();
                    drop(guard);
                    emit(
                        &inner,
                        ApiEvent::DocumentsChanged(DocumentsChangedEvent { documents }),
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn spawn_pipeline_listener(inner: Arc<Inner>) {
    let mut rx = pipeline::subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(progress) => {
                    update_job_state(&inner, pipeline_job_state(progress)).await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn spawn_download_listener(inner: Arc<Inner>) {
    let mut rx = download::subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(progress) => {
                    update_download_state(&inner, download_state(progress)).await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn spawn_llm_listener(inner: Arc<Inner>) {
    tokio::spawn(async move {
        let resources = loop {
            if let Ok(resources) = get_resources(&inner.shared) {
                break resources;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };

        let mut rx = resources.llm.subscribe();
        loop {
            match rx.recv().await {
                Ok(state) => emit(&inner, ApiEvent::LlmChanged(state)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn emit(inner: &Arc<Inner>, event: ApiEvent) {
    let _ = inner.tx.send(event);
}

async fn update_job_state(inner: &Arc<Inner>, job: JobState) {
    let terminal = !matches!(job.status, JobStatus::Running);
    {
        let mut jobs = inner.jobs.write().await;
        if terminal {
            jobs.remove(&job.id);
        } else {
            jobs.insert(job.id.clone(), job.clone());
        }
    }
    emit(inner, ApiEvent::JobChanged(job));
}

async fn update_download_state(inner: &Arc<Inner>, download: DownloadState) {
    let terminal = !matches!(
        download.status,
        TransferStatus::Started | TransferStatus::Downloading
    );
    {
        let mut downloads = inner.downloads.write().await;
        if terminal {
            downloads.remove(&download.id);
        } else {
            downloads.insert(download.id.clone(), download.clone());
        }
    }
    emit(inner, ApiEvent::DownloadChanged(download));
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

use koharu_core::DownloadProgress;

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
