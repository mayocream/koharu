use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::Phase;

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    changed: tokio::sync::watch::Sender<bool>,
}

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<CancellationState>);

impl CancellationToken {
    pub fn cancel(&self) {
        if !self.0.cancelled.swap(true, Ordering::AcqRel) {
            self.0.changed.send_replace(true);
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let mut changed = self.0.changed.subscribe();
        if *changed.borrow() {
            return;
        }
        let _ = changed.changed().await;
    }
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub phase: Phase,
    pub model: String,
    pub completed: usize,
    pub total: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WorkerState {
    Spawned,
    Loading,
    Ready,
    Running,
    Crashed,
}

#[derive(Clone, Debug)]
pub struct WorkerLifecycle {
    pub phase: Phase,
    pub model: String,
    pub generation: u64,
    pub state: WorkerState,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelMeasurement {
    pub phase: Phase,
    pub model: String,
    pub generation: u64,
    pub cold: bool,
    pub load: Option<std::time::Duration>,
    pub input_transfer: std::time::Duration,
    pub processor: std::time::Duration,
    pub output_transfer: std::time::Duration,
    pub round_trip: std::time::Duration,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub control_request_bytes: usize,
    pub control_response_bytes: usize,
}

#[derive(Clone, Debug)]
pub enum PipelineEvent {
    Progress(Progress),
    Download(koharu_runtime::download::Event),
    Worker(WorkerLifecycle),
    Measurement(ModelMeasurement),
}

pub type EventSink = Arc<dyn Fn(PipelineEvent) + Send + Sync>;
