use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use async_trait::async_trait;
use koharu_worker::{Emitter, Handler};

use super::wire::{ModelEvent, ModelRequest, ModelResponse, WireCommands};
use crate::{
    Processor, ProcessorFactory, WorkerState, builtin::BuiltinFactory, plan::ConfiguredModel,
    validate_processor,
};

struct ModelHandler {
    state: Option<ModelState>,
}

struct ModelState {
    model: ConfiguredModel,
    device: koharu_ml::Device,
    processor: Box<dyn Processor>,
    root: PathBuf,
    load_micros: Option<u64>,
}

#[async_trait]
impl Handler for ModelHandler {
    type Request = ModelRequest;
    type Response = ModelResponse;
    type Event = ModelEvent;

    async fn handle(
        &mut self,
        request: Self::Request,
        events: Emitter<Self::Event>,
    ) -> Result<Self::Response> {
        let ModelRequest {
            model,
            device,
            shared_root,
            context,
            blobs,
        } = request;
        if let Some(state) = &self.state {
            if state.model != model || state.device != device || state.root != shared_root {
                bail!("model worker received a request for a different model");
            }
        } else {
            events.emit(ModelEvent::State(WorkerState::Loading))?;
            let started = Instant::now();
            let processor =
                relay_downloads(BuiltinFactory.create(&model, device.clone()), &events).await?;
            validate_processor(&model, processor.as_ref())?;
            let load_micros = duration_micros(started.elapsed());
            self.state = Some(ModelState {
                model,
                device,
                processor,
                root: shared_root,
                load_micros: Some(load_micros),
            });
            events.emit(ModelEvent::State(WorkerState::Ready))?;
        }
        let state = self
            .state
            .as_mut()
            .context("model worker was not initialized")?;
        let input_bytes = blobs.byte_len()?;
        let context = context.into_context(&blobs, &state.root)?;
        events.emit(ModelEvent::State(WorkerState::Running))?;
        let started = Instant::now();
        let commands = relay_downloads(state.processor.run(&context), &events).await?;
        let processor_micros = duration_micros(started.elapsed());
        let output_root = state.root.clone();
        let (commands, attachments, output_bytes) = tokio::task::spawn_blocking(move || {
            WireCommands::from_commands(commands, &output_root)
        })
        .await??;
        debug_assert_eq!(state.model.inputs(), state.processor.inputs());
        debug_assert_eq!(state.model.outputs(), state.processor.outputs());
        let load_micros = state.load_micros.take();
        Ok(ModelResponse {
            commands,
            attachments,
            load_micros,
            processor_micros,
            input_bytes,
            output_bytes,
        })
    }
}

async fn relay_downloads<T>(
    future: impl std::future::Future<Output = Result<T>>,
    events: &Emitter<ModelEvent>,
) -> Result<T> {
    let mut downloads = koharu_runtime::download::subscribe();
    tokio::pin!(future);
    let result = loop {
        tokio::select! {
            result = &mut future => break result,
            event = downloads.recv() => match event {
                Ok(event) => events.emit(ModelEvent::Download(event))?,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "model worker download event subscriber fell behind");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break future.await,
            },
        }
    };
    loop {
        match downloads.try_recv() {
            Ok(event) => events.emit(ModelEvent::Download(event))?,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                tracing::warn!(
                    skipped,
                    "model worker download event subscriber fell behind"
                );
            }
            Err(
                tokio::sync::broadcast::error::TryRecvError::Empty
                | tokio::sync::broadcast::error::TryRecvError::Closed,
            ) => break,
        }
    }
    result
}

fn duration_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

pub(super) async fn serve() -> Result<()> {
    koharu_worker::serve(ModelHandler { state: None }).await
}
