use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use koharu_core::{
    DocumentChangedEvent, DocumentSummary, DocumentsChangedEvent, DownloadState, DownloadStatus,
    JobState, JobStatus, LlmState, PipelineProgress, PipelineStatus, PipelineStep, SnapshotEvent,
    TransferStatus,
};
use koharu_runtime::download;
use tokio::sync::broadcast;

use crate::server::state::{SharedResources, get_resources};
use crate::services::{pipeline::runner, store};

#[derive(Debug, Clone)]
pub(crate) enum ApiEvent {
    Documents(DocumentsChangedEvent),
    Document(DocumentChangedEvent),
    Job(JobState),
    Download(DownloadState),
    Llm(LlmState),
}

#[derive(Clone)]
pub(crate) struct EventHub {
    inner: Arc<Inner>,
}

struct Inner {
    shared: SharedResources,
    tx: broadcast::Sender<ApiEvent>,
    jobs: DashMap<String, JobState>,
    downloads: DashMap<String, DownloadState>,
}

impl EventHub {
    pub(crate) fn new(shared: SharedResources) -> Self {
        let inner = Arc::new(Inner {
            shared,
            tx: broadcast::channel(1024).0,
            jobs: DashMap::new(),
            downloads: DashMap::new(),
        });

        spawn_state_listener(inner.clone());
        spawn_pipeline_listener(inner.clone());
        spawn_download_listener(inner.clone());
        spawn_llm_listener(inner.clone());

        Self { inner }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<ApiEvent> {
        self.inner.tx.subscribe()
    }

    pub(crate) async fn snapshot(&self) -> anyhow::Result<SnapshotEvent> {
        let (documents, llm) = match get_resources(&self.inner.shared) {
            Ok(resources) => {
                let guard = resources.state.read().await;
                let documents = guard.documents.iter().map(DocumentSummary::from).collect();
                drop(guard);
                let llm = resources.llm.snapshot().await;
                (documents, llm)
            }
            Err(_) => (
                Vec::new(),
                LlmState {
                    status: koharu_core::LlmStateStatus::Empty,
                    model_id: None,
                    source: None,
                    error: None,
                },
            ),
        };

        let mut jobs = self
            .inner
            .jobs
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        jobs.sort_by(|left, right| left.id.cmp(&right.id));

        let mut downloads = self
            .inner
            .downloads
            .iter()
            .map(|entry| entry.value().clone())
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
    let mut rx = store::subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(store::StateEvent::Documents) => {
                    let Ok(resources) = get_resources(&inner.shared) else {
                        continue;
                    };
                    let guard = resources.state.read().await;
                    let documents = guard.documents.iter().map(DocumentSummary::from).collect();
                    drop(guard);
                    emit(
                        &inner,
                        ApiEvent::Documents(DocumentsChangedEvent { documents }),
                    );
                }
                Ok(store::StateEvent::Document {
                    document_id,
                    revision,
                    changed,
                }) => {
                    emit(
                        &inner,
                        ApiEvent::Document(DocumentChangedEvent {
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
                        ApiEvent::Documents(DocumentsChangedEvent { documents }),
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn spawn_pipeline_listener(inner: Arc<Inner>) {
    let mut rx = runner::subscribe();
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
                Ok(state) => emit(&inner, ApiEvent::Llm(state)),
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
    if terminal {
        inner.jobs.remove(&job.id);
    } else {
        inner.jobs.insert(job.id.clone(), job.clone());
    }
    emit(inner, ApiEvent::Job(job));
}

async fn update_download_state(inner: &Arc<Inner>, download: DownloadState) {
    let terminal = !matches!(
        download.status,
        TransferStatus::Started | TransferStatus::Downloading
    );
    if terminal {
        inner.downloads.remove(&download.id);
    } else {
        inner
            .downloads
            .insert(download.id.clone(), download.clone());
    }
    emit(inner, ApiEvent::Download(download));
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
        id: progress.id,
        label: progress.label,
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
    use std::sync::Arc;

    use tokio::sync::OnceCell;

    use koharu_core::{PipelineStatus, PipelineStep};

    use super::{EventHub, TransferStatus, download_state, pipeline_job_state};

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
            id: "hf:test:model.bin".to_string(),
            label: "model.bin".to_string(),
            filename: "model.bin".to_string(),
            downloaded: 32,
            total: Some(64),
            status: koharu_core::DownloadStatus::Completed,
        });

        assert_eq!(state.id, "hf:test:model.bin");
        assert_eq!(state.label, "model.bin");
        assert!(matches!(state.status, TransferStatus::Completed));
        assert_eq!(state.total, Some(64));
    }

    #[tokio::test]
    async fn snapshot_succeeds_before_resources_are_initialized() {
        let shared = Arc::new(OnceCell::new());
        let hub = EventHub::new(shared);

        let snapshot = hub.snapshot().await.expect("snapshot");
        assert!(snapshot.documents.is_empty());
        assert!(snapshot.jobs.is_empty());
        assert!(snapshot.downloads.is_empty());
    }
}
