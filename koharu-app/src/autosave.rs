//! Debounced scene.bin compactor task.
//!
//! Subscribes to `AutosaveSignal`s from wherever the scene mutates. Coalesces
//! a burst of signals into a single `compact()` after a short idle period,
//! plus a periodic tick to guarantee progress under continuous traffic.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::session::ProjectSession;

/// How long to wait after the last signal before flushing.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// Upper bound between flushes even if signals keep arriving.
const MAX_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
pub enum AutosaveSignal {
    /// A normal op was applied; coalesce for `DEBOUNCE`.
    Dirty,
    /// Flush now (e.g. project close, explicit compact request).
    FlushNow,
}

/// Handle to a running autosave loop. Dropping the sender lets the loop
/// drain and exit; `join` waits for that exit so the caller can ensure the
/// autosave's `Arc<ProjectSession>` has been released (useful when closing
/// a project and the fs4 lock must be freed before re-opening).
pub struct AutosaveHandle {
    pub tx: mpsc::Sender<AutosaveSignal>,
    pub join: tokio::task::JoinHandle<()>,
}

/// Spawn the autosave loop bound to `session`. Returns a handle containing
/// both the signal sender and the task's `JoinHandle`.
pub fn spawn(session: Arc<ProjectSession>) -> AutosaveHandle {
    let (tx, mut rx) = mpsc::channel::<AutosaveSignal>(64);
    let join = tokio::spawn(async move {
        let mut dirty = false;
        let mut interval = tokio::time::interval(MAX_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Consume the immediate first tick.
        let _ = interval.tick().await;

        loop {
            tokio::select! {
                maybe = rx.recv() => {
                    match maybe {
                        Some(AutosaveSignal::FlushNow) => {
                            if let Err(err) = compact_blocking(&session).await {
                                tracing::error!(?err, "autosave: compact failed");
                            }
                            dirty = false;
                        }
                        Some(AutosaveSignal::Dirty) => {
                            dirty = true;
                            // Debounce: wait for either another signal or the timeout.
                            let timeout = tokio::time::sleep(DEBOUNCE);
                            tokio::pin!(timeout);
                            loop {
                                tokio::select! {
                                    more = rx.recv() => match more {
                                        Some(AutosaveSignal::FlushNow) | None => {
                                            // FlushNow or channel closed: compact below.
                                            break;
                                        }
                                        Some(AutosaveSignal::Dirty) => {
                                            dirty = true;
                                            timeout.as_mut().reset(tokio::time::Instant::now() + DEBOUNCE);
                                        }
                                    },
                                    _ = &mut timeout => break,
                                }
                            }
                            if dirty
                                && let Err(err) = compact_blocking(&session).await
                            {
                                tracing::error!(?err, "autosave: compact failed");
                            }
                            dirty = false;
                        }
                        None => break,
                    }
                }
                _ = interval.tick(), if dirty => {
                    if let Err(err) = compact_blocking(&session).await {
                        tracing::error!(?err, "autosave: periodic compact failed");
                    }
                    dirty = false;
                }
            }
        }
    });
    AutosaveHandle { tx, join }
}

/// Run the blocking `compact()` off the async runtime.
async fn compact_blocking(session: &Arc<ProjectSession>) -> anyhow::Result<()> {
    let session = session.clone();
    tokio::task::spawn_blocking(move || session.compact()).await?
}
