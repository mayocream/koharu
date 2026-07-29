mod export;
mod import;
mod pipeline;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use crate::protocol::{ExportFormat, RequestId};
use anyhow::{Result, anyhow};
use koharu_config::Config;
use koharu_desktop::DesktopHandle;
use koharu_pipeline::{Operation, Pipeline, PipelineConfig, Scope, Stage, StopToken};
use koharu_renderer::Renderer;
use koharu_scene::{EntityId, LanguageTag, Revision, SceneSnapshot};
use tokio::{sync::mpsc, task::JoinHandle};

pub enum NativeEvent {
    RuntimeInitialized,
    RuntimeInitializationFailed {
        error: String,
        retry_after_ms: u64,
    },
    Download(koharu_runtime::download::Event),
    Resources(koharu_pipeline::ResourceSnapshot),
    PipelineProgress {
        job: RequestId,
        completed: usize,
        total: usize,
        page: Option<EntityId>,
        stage: Option<Stage>,
        model: Option<String>,
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
    Finished {
        job: RequestId,
        revisions: Vec<Revision>,
        pages: Vec<EntityId>,
        stopped: bool,
        error: Option<String>,
    },
}

pub struct PipelineRequest {
    pub id: RequestId,
    pub path: PathBuf,
    pub scope: Scope,
    pub operation: Operation,
}

pub struct ExportRequest {
    pub id: RequestId,
    pub snapshot: SceneSnapshot,
    pub directory: PathBuf,
    pub pages: Vec<EntityId>,
    pub format: ExportFormat,
    pub locale: Option<LanguageTag>,
}

struct ExportJob {
    request: ExportRequest,
    stop: StopToken,
    desktop: DesktopHandle<NativeEvent>,
}

pub struct Background {
    pipeline: RwLock<Arc<Pipeline>>,
    exports: mpsc::UnboundedSender<ExportJob>,
    export_worker: JoinHandle<()>,
    jobs: Mutex<Vec<JoinHandle<()>>>,
    download_worker: Option<JoinHandle<()>>,
    resource_worker: Option<JoinHandle<()>>,
}

impl Background {
    pub fn new(
        config: Config<PipelineConfig>,
        translation: Config<koharu_translator::TranslationConfig>,
    ) -> Result<Self> {
        let pipeline = Arc::new(Pipeline::new(
            config.read()?.clone(),
            translation.read()?.clone(),
        )?);
        let (exports, receiver) = mpsc::unbounded_channel();
        let export_worker = tokio::spawn(run_exports(receiver));
        Ok(Self {
            pipeline: RwLock::new(pipeline),
            exports,
            export_worker,
            jobs: Mutex::new(Vec::new()),
            download_worker: None,
            resource_worker: None,
        })
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

    pub fn subscribe_resources(&mut self, desktop: DesktopHandle<NativeEvent>) {
        if self.resource_worker.is_some() {
            return;
        }

        let mut resources = self
            .pipeline
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .subscribe_resources();
        self.resource_worker = Some(tokio::spawn(async move {
            loop {
                if resources.changed().await.is_err() {
                    break;
                }
                let snapshot = resources.borrow_and_update().clone();
                if desktop
                    .send_event(NativeEvent::Resources(snapshot))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub fn run_pipeline(
        &self,
        request: PipelineRequest,
        desktop: DesktopHandle<NativeEvent>,
    ) -> Result<StopToken> {
        let stop = StopToken::default();
        let pipeline = self
            .pipeline
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let job_stop = stop.clone();
        self.track(tokio::spawn(async move {
            pipeline::run(&pipeline, request, job_stop, desktop).await;
        }));
        Ok(stop)
    }

    pub fn reconfigure(
        &self,
        config: PipelineConfig,
        translation: koharu_translator::TranslationConfig,
    ) -> Result<()> {
        let replacement = {
            let pipeline = self
                .pipeline
                .read()
                .unwrap_or_else(|error| error.into_inner());
            Arc::new(pipeline.reconfigured(config, translation)?)
        };
        *self
            .pipeline
            .write()
            .unwrap_or_else(|error| error.into_inner()) = replacement;
        Ok(())
    }

    pub fn import(
        &self,
        id: RequestId,
        path: PathBuf,
        files: Vec<PathBuf>,
        desktop: DesktopHandle<NativeEvent>,
    ) -> Result<StopToken> {
        let stop = StopToken::default();
        let job_stop = stop.clone();
        self.track(tokio::task::spawn_blocking(move || {
            import::run(id, path, files, job_stop, desktop);
        }));
        Ok(stop)
    }

    pub fn export(
        &self,
        request: ExportRequest,
        desktop: DesktopHandle<NativeEvent>,
    ) -> Result<StopToken> {
        let stop = StopToken::default();
        self.exports
            .send(ExportJob {
                request,
                stop: stop.clone(),
                desktop,
            })
            .map_err(|_| anyhow!("export runner has stopped"))?;
        Ok(stop)
    }

    fn track(&self, worker: JoinHandle<()>) {
        let mut jobs = self.jobs.lock().unwrap_or_else(|error| error.into_inner());
        jobs.retain(|job| !job.is_finished());
        jobs.push(worker);
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        if let Some(worker) = self.download_worker.take() {
            worker.abort();
        }
        if let Some(worker) = self.resource_worker.take() {
            worker.abort();
        }
        self.export_worker.abort();
        for job in self
            .jobs
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
        {
            job.abort();
        }
    }
}

async fn run_exports(mut receiver: mpsc::UnboundedReceiver<ExportJob>) {
    let mut renderer = None::<Renderer>;
    while let Some(job) = receiver.recv().await {
        tokio::task::block_in_place(|| {
            export::run(&mut renderer, job.request, job.stop, job.desktop);
        });
    }
}

#[derive(Default)]
struct JobOutcome {
    revisions: Vec<Revision>,
    pages: Vec<EntityId>,
    stopped: bool,
    error: Option<String>,
}

fn finish_job(desktop: &DesktopHandle<NativeEvent>, job: RequestId, outcome: JobOutcome) {
    let _ = desktop.send_event(NativeEvent::Finished {
        job,
        revisions: outcome.revisions,
        pages: outcome.pages,
        stopped: outcome.stopped,
        error: outcome.error,
    });
}
