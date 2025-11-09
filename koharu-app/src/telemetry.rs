use std::sync::{Arc, Mutex};

use koharu_core::progress::{PROGRESS_TRACE_TARGET, PROGRESS_WINDOW_EVENT, ProgressEvent};
use tauri::{AppHandle, Emitter};
use tracing::{Event, Subscriber};
use tracing_subscriber::{
    filter::LevelFilter,
    layer::{Context as LayerContext, Layer},
    prelude::*,
};

pub fn init(handle: Arc<Mutex<AppHandle>>) -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(LevelFilter::DEBUG))
        .with(ProgressLayer::new(handle))
        .try_init()?;

    Ok(())
}

struct ProgressLayer {
    handle: Arc<Mutex<AppHandle>>,
}

impl ProgressLayer {
    fn new(handle: Arc<Mutex<AppHandle>>) -> Self {
        Self { handle }
    }
}

impl<S> Layer<S> for ProgressLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: LayerContext<'_, S>) {
        if event.metadata().target() != PROGRESS_TRACE_TARGET {
            return;
        }

        let mut visitor = ProgressVisitor::default();
        event.record(&mut visitor);

        if let Some(payload) = visitor.finish() {
            emit_to_window(&self.handle, &payload);
        }
    }
}

#[derive(Default)]
struct ProgressVisitor {
    kind: Option<String>,
    url: Option<String>,
    current: Option<u64>,
    total: Option<u64>,
}

impl ProgressVisitor {
    fn finish(self) -> Option<ProgressEvent> {
        let kind = self.kind?;
        let url = self.url?;

        match kind.as_str() {
            "started" => Some(ProgressEvent::Started {
                url,
                total: self.total? as usize,
            }),
            "progress" => Some(ProgressEvent::Progress {
                url,
                current: self.current? as usize,
                total: self.total? as usize,
            }),
            "finished" => Some(ProgressEvent::Finished { url }),
            _ => None,
        }
    }
}

impl tracing::field::Visit for ProgressVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "kind" => self.kind = Some(value.to_owned()),
            "url" => self.url = Some(value.to_owned()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.record_str(field, &format!("{value:?}"));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        match field.name() {
            "current" => self.current = Some(value),
            "total" => self.total = Some(value),
            _ => {}
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if value >= 0 {
            self.record_u64(field, value as u64);
        }
    }
}

fn emit_to_window(handle: &Arc<Mutex<AppHandle>>, event: &ProgressEvent) {
    let Ok(app) = handle.lock() else {
        return;
    };

    if let Err(err) = app.emit(PROGRESS_WINDOW_EVENT, event) {
        tracing::warn!(
            target: PROGRESS_TRACE_TARGET,
            "failed to emit download event: {err}"
        );
    }
}
