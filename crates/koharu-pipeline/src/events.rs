use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use koharu_scene::Revision;

use crate::{ConfigRevision, Stage};

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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RunId(pub u64);

#[derive(Clone, Debug)]
pub enum UnloadReason {
    MemoryPressure,
    ConfigurationChanged,
    OutOfMemoryRecovery,
    FailureRecovery,
    PipelineDropped,
}

#[derive(Clone, Debug)]
pub enum PipelineEvent {
    ConfigurationChanged {
        generation: ConfigRevision,
        changed: Vec<Stage>,
    },
    RunStarted {
        run: RunId,
        base: Revision,
        stages: Vec<Stage>,
    },
    ModelLoadStarted {
        run: RunId,
        stage: Stage,
        model: String,
    },
    ModelLoadFinished {
        run: RunId,
        stage: Stage,
        model: String,
        elapsed: std::time::Duration,
    },
    ModelUnloaded {
        stage: Stage,
        model: String,
        reason: UnloadReason,
    },
    StageStarted {
        run: RunId,
        stage: Stage,
    },
    StageProgress {
        run: RunId,
        stage: Stage,
        completed: u64,
        total: Option<u64>,
    },
    StageFinished {
        run: RunId,
        stage: Stage,
        elapsed: std::time::Duration,
    },
    RunFinished {
        run: RunId,
        elapsed: std::time::Duration,
    },
    RunCancelled {
        run: RunId,
    },
    RunFailed {
        run: RunId,
        stage: Option<Stage>,
        message: String,
    },
}

pub type EventSink = Arc<dyn Fn(PipelineEvent) + Send + Sync>;

pub(crate) struct EventHub {
    next_run: AtomicU64,
    events: tokio::sync::broadcast::Sender<PipelineEvent>,
}

impl EventHub {
    pub(crate) fn new() -> Self {
        let (events, _) = tokio::sync::broadcast::channel(256);
        Self {
            next_run: AtomicU64::new(1),
            events,
        }
    }

    pub(crate) fn next_run(&self) -> RunId {
        RunId(self.next_run.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn emit(&self, event: PipelineEvent) {
        let _ = self.events.send(event);
    }

    pub(crate) fn emit_to(&self, sink: Option<&EventSink>, event: PipelineEvent) {
        self.emit(event.clone());
        if let Some(sink) = sink
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(event))).is_err()
        {
            tracing::warn!("pipeline event sink panicked");
        }
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::broadcast::Receiver<PipelineEvent> {
        self.events.subscribe()
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}
