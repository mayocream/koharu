use std::sync::Arc;

use crate::Stage;
use koharu_scene::EntityId;

#[derive(Clone, Debug)]
pub enum Progress {
    Started {
        pages: Vec<EntityId>,
        stages: Vec<Stage>,
    },
    Loading {
        page: EntityId,
        stage: Stage,
        model: String,
    },
    Running {
        page: EntityId,
        stage: Stage,
        model: String,
    },
    Finished {
        page: EntityId,
        stage: Stage,
        model: String,
        elapsed: std::time::Duration,
    },
    Skipped {
        page: EntityId,
        stage: Stage,
    },
}

pub type ProgressSink = Arc<dyn Fn(Progress) + Send + Sync>;

pub(crate) fn emit(sink: Option<&ProgressSink>, progress: Progress) {
    if let Some(sink) = sink
        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(progress))).is_err()
    {
        tracing::warn!("pipeline progress callback panicked");
    }
}
