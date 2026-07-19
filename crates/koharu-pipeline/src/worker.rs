mod server;
mod wire;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};
use async_trait::async_trait;
use koharu_scene::Commands;
use koharu_worker::{CallError, Client};
use tempfile::TempDir;

use self::wire::{ModelEvent, ModelRequest, ModelResponse, SharedBlobs, WireContext};
use crate::{
    Artifact, Context, EventSink, ModelMeasurement, Phase, PipelineEvent, Processor,
    ProcessorFactory, WorkerLifecycle, WorkerState, plan::ConfiguredModel,
};

const WORKER_ARGUMENT: &str = "--worker";

#[derive(Default)]
pub(crate) struct WorkerFactory {
    root: Mutex<Option<Arc<TempDir>>>,
    executable: Option<PathBuf>,
}

impl WorkerFactory {
    pub(crate) fn with_executable(executable: PathBuf) -> Self {
        Self {
            root: Mutex::new(None),
            executable: Some(executable),
        }
    }
}

#[async_trait]
impl ProcessorFactory for WorkerFactory {
    async fn create(
        &self,
        model: &ConfiguredModel,
        device: koharu_ml::Device,
    ) -> Result<Box<dyn Processor>> {
        let root = {
            let mut root = self
                .root
                .lock()
                .map_err(|_| anyhow!("model worker directory lock is poisoned"))?;
            if root.is_none() {
                *root = Some(Arc::new(
                    tempfile::Builder::new()
                        .prefix("koharu-workers-")
                        .tempdir()
                        .context("failed to create the model worker directory")?,
                ));
            }
            root.as_ref().expect("worker root was initialized").clone()
        };
        Ok(Box::new(WorkerProcessor {
            model: model.clone(),
            device,
            root,
            executable: self.executable.clone(),
            client: None,
        }))
    }
}

struct WorkerProcessor {
    model: ConfiguredModel,
    device: koharu_ml::Device,
    root: Arc<TempDir>,
    executable: Option<PathBuf>,
    client: Option<Client>,
}

#[async_trait]
impl Processor for WorkerProcessor {
    fn name(&self) -> &'static str {
        self.model.name()
    }

    fn inputs(&self) -> &'static [Artifact] {
        self.model.inputs()
    }

    fn outputs(&self) -> &'static [Artifact] {
        self.model.outputs()
    }

    async fn shutdown(&mut self) {
        if let Some(client) = self.client.take() {
            client.shutdown().await;
        }
    }

    async fn run(&mut self, context: &Context) -> Result<Commands> {
        let input_transfer_started = Instant::now();
        let shared_context = context.clone();
        let shared_root = self.root.path().to_path_buf();
        let snapshot =
            tokio::task::spawn_blocking(move || shared_context.shared_snapshot(&shared_root))
                .await??;
        let input_transfer = input_transfer_started.elapsed();
        let request = ModelRequest {
            model: self.model.clone(),
            device: self.device.clone(),
            shared_root: self.root.path().to_path_buf(),
            context: WireContext::from_context(context),
            blobs: SharedBlobs {
                arena: snapshot.descriptor.clone(),
                entries: snapshot.blobs.clone(),
            },
        };
        let spawned = self.client.is_none();
        if spawned {
            self.client = Some(self.spawn().await?);
        }
        let client = self
            .client
            .as_mut()
            .expect("model worker client was created above");
        let generation = client.generation();
        let mut events = WorkerEventRelay::new(
            context.event_sink(),
            context.phase(),
            &self.model,
            generation,
        );
        if spawned {
            events.lifecycle(WorkerState::Spawned, None);
        }
        let result = client
            .call(&request, context.cancellation().cancelled(), |event| {
                events.handle(event)
            })
            .await;
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                events.fail_downloads(&error.to_string());
                if matches!(error, CallError::Crashed(_)) {
                    events.lifecycle(WorkerState::Crashed, Some(error.to_string()));
                }
                if !matches!(error, CallError::Remote(_)) {
                    self.client.take();
                }
                return Err(call_error(error));
            }
        };
        events.fail_downloads("model request ended before the download finished");
        let ModelResponse {
            commands,
            attachments,
            load_micros,
            processor_micros,
            input_bytes,
            output_bytes,
        } = result.value;
        let output_transfer_started = Instant::now();
        let output_root = self.root.path().to_path_buf();
        let commands =
            tokio::task::spawn_blocking(move || commands.into_commands(attachments, &output_root))
                .await??;
        let load = load_micros.map(Duration::from_micros);
        context.record_measurement(ModelMeasurement {
            phase: context.phase(),
            model: self.model.name().to_owned(),
            generation: result.metrics.generation,
            cold: load.is_some(),
            load,
            input_transfer,
            processor: Duration::from_micros(processor_micros),
            output_transfer: output_transfer_started.elapsed(),
            round_trip: result.metrics.round_trip,
            input_bytes,
            output_bytes,
            control_request_bytes: result.metrics.request_bytes,
            control_response_bytes: result.metrics.response_bytes,
        });
        Ok(commands)
    }
}

impl WorkerProcessor {
    async fn spawn(&self) -> Result<Client> {
        match &self.executable {
            Some(executable) => Client::spawn_executable(executable, WORKER_ARGUMENT).await,
            None => Client::spawn(WORKER_ARGUMENT).await,
        }
        .context("failed to spawn the model worker")
    }
}

fn call_error(error: CallError) -> anyhow::Error {
    match error {
        CallError::Cancelled => anyhow!("pipeline run was cancelled"),
        error => anyhow!(error),
    }
}

struct WorkerEventRelay {
    sink: Option<EventSink>,
    phase: Phase,
    model: String,
    generation: u64,
    downloads: HashMap<u64, String>,
}

impl WorkerEventRelay {
    fn new(
        sink: Option<EventSink>,
        phase: Phase,
        model: &ConfiguredModel,
        generation: u64,
    ) -> Self {
        Self {
            sink,
            phase,
            model: model.name().to_owned(),
            generation,
            downloads: HashMap::new(),
        }
    }

    fn lifecycle(&self, state: WorkerState, detail: Option<String>) {
        if let Some(sink) = &self.sink {
            sink(PipelineEvent::Worker(WorkerLifecycle {
                phase: self.phase,
                model: self.model.clone(),
                generation: self.generation,
                state,
                detail,
            }));
        }
    }

    fn handle(&mut self, event: ModelEvent) {
        match event {
            ModelEvent::State(state) => self.lifecycle(state, None),
            ModelEvent::Download(event) => {
                let event = namespace_download(self.generation, event);
                match &event {
                    koharu_runtime::download::Event::Started { id, name }
                    | koharu_runtime::download::Event::Progress { id, name, .. } => {
                        self.downloads.insert(*id, name.clone());
                    }
                    koharu_runtime::download::Event::Finished { id }
                    | koharu_runtime::download::Event::Failed { id, .. } => {
                        self.downloads.remove(id);
                    }
                }
                if let Some(sink) = &self.sink {
                    sink(PipelineEvent::Download(event));
                }
            }
        }
    }

    fn fail_downloads(&mut self, error: &str) {
        let Some(sink) = &self.sink else {
            self.downloads.clear();
            return;
        };
        for (id, name) in self.downloads.drain() {
            sink(PipelineEvent::Download(
                koharu_runtime::download::Event::Failed {
                    id,
                    name,
                    error: error.to_owned(),
                },
            ));
        }
    }
}

fn namespace_download(
    generation: u64,
    event: koharu_runtime::download::Event,
) -> koharu_runtime::download::Event {
    let id = |local: u64| (generation << 32) ^ (local & u64::from(u32::MAX));
    match event {
        koharu_runtime::download::Event::Started { id: local, name } => {
            koharu_runtime::download::Event::Started {
                id: id(local),
                name,
            }
        }
        koharu_runtime::download::Event::Progress {
            id: local,
            name,
            completed,
            total,
        } => koharu_runtime::download::Event::Progress {
            id: id(local),
            name,
            completed,
            total,
        },
        koharu_runtime::download::Event::Finished { id: local } => {
            koharu_runtime::download::Event::Finished { id: id(local) }
        }
        koharu_runtime::download::Event::Failed {
            id: local,
            name,
            error,
        } => koharu_runtime::download::Event::Failed {
            id: id(local),
            name,
            error,
        },
    }
}

pub async fn serve_worker() -> Result<()> {
    server::serve().await
}
