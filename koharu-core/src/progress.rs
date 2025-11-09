use serde::Serialize;
use tracing::{Level, event};

pub const PROGRESS_TRACE_TARGET: &str = "koharu::progress";
pub const PROGRESS_WINDOW_EVENT: &str = "download://progress";

// refer: https://v2.tauri.app/develop/calling-frontend/#channels
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "event", content = "data")]
pub enum ProgressEvent {
    Started {
        url: String,
        total: usize,
    },
    Progress {
        url: String,
        current: usize,
        total: usize,
    },
    Finished {
        url: String,
    },
}

#[derive(Clone)]
pub struct Emitter {
    url: String,
    total: usize,
    current: usize,
}

impl Emitter {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            total: 0,
            current: 0,
        }
    }

    pub fn begin(&mut self, total: usize) {
        self.total = total;
        self.current = 0;
        event!(
            target: PROGRESS_TRACE_TARGET,
            Level::TRACE,
            kind = "started",
            url = %self.url,
            total = total as u64
        );
    }

    pub fn advance(&mut self, delta: usize) {
        if delta == 0 {
            return;
        }

        self.current = self.current.saturating_add(delta);
        event!(
            target: PROGRESS_TRACE_TARGET,
            Level::TRACE,
            kind = "progress",
            url = %self.url,
            current = self.current as u64,
            total = self.total as u64
        );
    }

    pub fn complete(&mut self) {
        event!(
            target: PROGRESS_TRACE_TARGET,
            Level::TRACE,
            kind = "finished",
            url = %self.url
        );
    }
}

impl hf_hub::api::tokio::Progress for Emitter {
    async fn init(&mut self, size: usize, _filename: &str) {
        self.begin(size);
    }

    async fn update(&mut self, size: usize) {
        self.advance(size);
    }

    async fn finish(&mut self) {
        self.complete();
    }
}
