mod export;
mod import;
mod pipeline;

use std::path::PathBuf;

use crate::protocol::{ExportFormat, RequestId};
use anyhow::{Result, anyhow};
use koharu_config::Config;
use koharu_desktop::DesktopHandle;
use koharu_pipeline::{CancellationToken, Force, Pipeline, PipelineConfig, RunTarget, Scope};
use koharu_renderer::Renderer;
use koharu_scene::{PageId, Revision};
use koharu_translator::TranslationConfig;
use tokio::{sync::mpsc, task::JoinHandle};

pub enum NativeEvent {
    Download(koharu_runtime::download::Event),
    PipelineProgress {
        job: RequestId,
        progress: koharu_pipeline::Progress,
    },
    ImportProgress {
        job: RequestId,
        completed: usize,
        total: usize,
    },
    ExportProgress {
        job: RequestId,
        completed: usize,
        total: usize,
    },
    ProjectAdvanced {
        job: RequestId,
    },
    FontCached {
        family: String,
        weight: u16,
        italic: bool,
        error: Option<String>,
    },
    Finished {
        job: RequestId,
        revisions: Vec<Revision>,
        pages: Vec<PageId>,
        cancelled: bool,
        error: Option<String>,
    },
}

pub struct PipelineRequest {
    pub id: RequestId,
    pub path: PathBuf,
    pub scope: Scope,
    pub target: RunTarget,
    pub force: Force,
}

pub struct ExportRequest {
    pub id: RequestId,
    pub path: PathBuf,
    pub directory: PathBuf,
    pub pages: Vec<PageId>,
    pub format: ExportFormat,
}

enum Job {
    Pipeline {
        request: PipelineRequest,
        cancellation: CancellationToken,
        desktop: DesktopHandle<NativeEvent>,
    },
    Import {
        id: RequestId,
        path: PathBuf,
        files: Vec<PathBuf>,
        cancellation: CancellationToken,
        desktop: DesktopHandle<NativeEvent>,
    },
    Export {
        request: ExportRequest,
        cancellation: CancellationToken,
        desktop: DesktopHandle<NativeEvent>,
    },
}

pub struct Background {
    sender: mpsc::UnboundedSender<Job>,
    worker: JoinHandle<()>,
    download_worker: Option<JoinHandle<()>>,
}

impl Background {
    pub fn new(config: Config<PipelineConfig>, translation: Config<TranslationConfig>) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let worker = tokio::spawn(run_jobs(receiver, config, translation));
        Self {
            sender,
            worker,
            download_worker: None,
        }
    }

    pub fn subscribe_downloads(&mut self, desktop: DesktopHandle<NativeEvent>) {
        if self.download_worker.is_some() {
            return;
        }

        let mut events = koharu_runtime::download::subscribe();
        self.download_worker = Some(tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        if desktop.send_event(NativeEvent::Download(event)).is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "download event subscriber fell behind");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }));
    }

    pub fn run_pipeline(
        &self,
        request: PipelineRequest,
        desktop: DesktopHandle<NativeEvent>,
    ) -> Result<CancellationToken> {
        self.submit(|cancellation| Job::Pipeline {
            request,
            cancellation,
            desktop,
        })
    }

    pub fn import(
        &self,
        id: RequestId,
        path: PathBuf,
        files: Vec<PathBuf>,
        desktop: DesktopHandle<NativeEvent>,
    ) -> Result<CancellationToken> {
        self.submit(|cancellation| Job::Import {
            id,
            path,
            files,
            cancellation,
            desktop,
        })
    }

    pub fn export(
        &self,
        request: ExportRequest,
        desktop: DesktopHandle<NativeEvent>,
    ) -> Result<CancellationToken> {
        self.submit(|cancellation| Job::Export {
            request,
            cancellation,
            desktop,
        })
    }

    fn submit(&self, job: impl FnOnce(CancellationToken) -> Job) -> Result<CancellationToken> {
        let cancellation = CancellationToken::default();
        self.sender
            .send(job(cancellation.clone()))
            .map_err(|_| anyhow!("job runner has stopped"))?;
        Ok(cancellation)
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        if let Some(worker) = self.download_worker.take() {
            worker.abort();
        }
        self.worker.abort();
    }
}

async fn run_jobs(
    mut receiver: mpsc::UnboundedReceiver<Job>,
    config: Config<PipelineConfig>,
    translation: Config<TranslationConfig>,
) {
    let mut renderer = None::<Renderer>;
    let pipeline = Pipeline::new(config, translation);
    while let Some(job) = receiver.recv().await {
        match job {
            Job::Pipeline {
                request,
                cancellation,
                desktop,
            } => pipeline::run(&pipeline, request, cancellation, desktop).await,
            Job::Import {
                id,
                path,
                files,
                cancellation,
                desktop,
            } => tokio::task::block_in_place(|| {
                import::run(id, path, files, cancellation, desktop);
            }),
            Job::Export {
                request,
                cancellation,
                desktop,
            } => tokio::task::block_in_place(|| {
                export::run(&mut renderer, request, cancellation, desktop);
            }),
        }
    }
    let _ = pipeline.unload_all().await;
}

#[derive(Default)]
struct JobOutcome {
    revisions: Vec<Revision>,
    pages: Vec<PageId>,
    error: Option<String>,
}

fn finish_job(
    desktop: &DesktopHandle<NativeEvent>,
    job: RequestId,
    cancellation: &CancellationToken,
    outcome: JobOutcome,
) {
    let _ = desktop.send_event(NativeEvent::Finished {
        job,
        revisions: outcome.revisions,
        pages: outcome.pages,
        cancelled: cancellation.is_cancelled(),
        error: outcome.error,
    });
}
