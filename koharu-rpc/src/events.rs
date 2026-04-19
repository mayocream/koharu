//! `GET /events` — Server-Sent Events stream.
//!
//! ## Delivery contract
//!
//! 1. **Fresh connect** (no `Last-Event-ID` header): the first frame is an
//!    `AppEvent::Snapshot` holding the current jobs + downloads registries.
//!    The client uses it to seed/replace its in-memory mirrors.
//! 2. **Reconnect** (`Last-Event-ID: <seq>` — browsers attach this
//!    automatically): we replay every buffered event with `seq > last_id`
//!    in ascending order before switching to live delivery. If the
//!    requested id predates the ring window, we fall back to `Snapshot`
//!    to re-seed state.
//! 3. **Live tail**: each subsequent event is serialised with its `seq` as
//!    the SSE `id:` field so reconnects remain lossless within the ring
//!    window.
//! 4. **Lag recovery**: if a live subscriber falls behind the broadcast
//!    buffer, axum's stream emits `BroadcastStreamRecvError::Lagged`; we
//!    respond with a fresh `Snapshot` instead of terminating the stream.

use async_stream::try_stream;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use koharu_app::bus::SequencedEvent;
use koharu_core::{AppEvent, DownloadProgress, JobSummary, SnapshotEvent};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default().routes(routes!(events))
}

/// Build the current registry snapshot.
fn snapshot_from(app: &AppState) -> SnapshotEvent {
    let jobs: Vec<JobSummary> = app.jobs.iter().map(|e| e.value().clone()).collect();
    let downloads: Vec<DownloadProgress> =
        app.downloads.iter().map(|e| e.value().clone()).collect();
    SnapshotEvent { jobs, downloads }
}

/// Parse `Last-Event-ID` header into a u64 seq, if present + valid.
fn last_event_id(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

#[utoipa::path(get, path = "/events", responses((status = 200, body = AppEvent)))]
async fn events(
    State(app): State<AppState>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, axum::Error>>> {
    let resume_from = last_event_id(&headers);
    let mut rx = BroadcastStream::new(app.bus.subscribe());
    let app_for_stream = app.clone();

    let stream = try_stream! {
        match resume_from {
            Some(last_id) => {
                // Reconnect: replay anything the client missed that's still
                // in the ring buffer. If there's a gap (the buffer no longer
                // contains `last_id + 1`), emit a snapshot first so the client
                // can rebuild state — then stream whatever tail we still have.
                let replay = app.bus.replay_since(last_id);
                let has_gap = match replay.first() {
                    Some(first) => first.seq > last_id + 1,
                    None => app.bus.latest_seq().map(|s| s > last_id).unwrap_or(false),
                };
                if has_gap {
                    yield snapshot_frame(&app)?;
                }
                for sev in replay {
                    yield sequenced_frame(&sev)?;
                }
            }
            None => {
                // Fresh connect: seed with a snapshot.
                yield snapshot_frame(&app)?;
            }
        }

        while let Some(msg) = rx.next().await {
            match msg {
                Ok(sev) => yield sequenced_frame(&sev)?,
                Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_n)) => {
                    // Subscriber fell off the broadcast window. Re-seed
                    // with a snapshot so the client can rebuild state.
                    yield snapshot_frame(&app_for_stream)?;
                }
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn snapshot_frame(app: &AppState) -> Result<Event, axum::Error> {
    let snap = snapshot_from(app);
    Event::default()
        .json_data(AppEvent::Snapshot(snap))
        .map_err(axum::Error::new)
}

fn sequenced_frame(sev: &SequencedEvent) -> Result<Event, axum::Error> {
    Event::default()
        .id(sev.seq.to_string())
        .json_data(&sev.event)
        .map_err(axum::Error::new)
}
