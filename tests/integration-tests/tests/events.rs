//! SSE `/events` end-to-end contract tests.
//!
//! Covers:
//! - Initial `Snapshot` on fresh connect.
//! - Live-delivery of `JobStarted` / `JobProgress` / `JobFinished` driven
//!   by a real `POST /pipelines` roundtrip.
//! - SSE `id:` field is the monotonic `seq`.
//! - `Last-Event-ID` reconnect replays missed events from the ring buffer.
//! - Lag fallback: a subscriber that falls off the broadcast buffer is
//!   re-seeded with a fresh `Snapshot` instead of terminating.
//! - Scene/project/config/LLM mutations do *not* broadcast — the bus is
//!   scoped strictly to long-running processes (regression guard).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::StreamExt;
use koharu_core::{AppEvent, JobFinishedEvent, JobStatus};
use koharu_integration_tests::TestApp;
use reqwest::multipart::{Form, Part};
use serde_json::Value;
use tokio::time::timeout;

// ---------------------------------------------------------------------------
// Tiny SSE parser
// ---------------------------------------------------------------------------

/// One parsed SSE frame.
#[derive(Debug, Default, Clone)]
struct Frame {
    id: Option<String>,
    data: String,
}

impl Frame {
    fn json(&self) -> Option<Value> {
        serde_json::from_str(&self.data).ok()
    }
    fn event_tag(&self) -> Option<String> {
        self.json()
            .and_then(|v| v.get("event").and_then(Value::as_str).map(str::to_string))
    }
}

/// Stream adapter that yields one [`Frame`] per `\n\n`-terminated chunk.
struct FrameStream {
    buf: String,
    bytes: reqwest::Response,
}

impl FrameStream {
    fn new(resp: reqwest::Response) -> Self {
        Self {
            buf: String::new(),
            bytes: resp,
        }
    }

    async fn next_frame(&mut self) -> Result<Option<Frame>> {
        loop {
            if let Some(idx) = self.buf.find("\n\n") {
                let raw = self.buf[..idx].to_string();
                self.buf.drain(..idx + 2);
                if raw.trim().is_empty() {
                    continue;
                }
                return Ok(Some(parse_frame(&raw)));
            }
            match self.bytes.chunk().await? {
                Some(chunk) => {
                    self.buf.push_str(std::str::from_utf8(&chunk)?);
                }
                None => return Ok(None),
            }
        }
    }
}

fn parse_frame(raw: &str) -> Frame {
    let mut frame = Frame::default();
    let mut data_lines: Vec<&str> = Vec::new();
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("id:") {
            frame.id = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim());
        }
    }
    frame.data = data_lines.join("\n");
    frame
}

// ---------------------------------------------------------------------------
// Helpers for kicking the bus directly (fast, deterministic)
// ---------------------------------------------------------------------------

fn job_started(id: &str) -> AppEvent {
    AppEvent::JobStarted {
        id: id.to_string(),
        kind: "pipeline".to_string(),
    }
}

fn job_finished(id: &str) -> AppEvent {
    AppEvent::JobFinished(JobFinishedEvent {
        id: id.to_string(),
        status: JobStatus::Completed,
        error: None,
    })
}

fn llm_unloaded() -> AppEvent {
    AppEvent::LlmUnloaded
}

async fn open_stream(app: &TestApp, last_event_id: Option<&str>) -> Result<FrameStream> {
    let mut req = app
        .client_config
        .client
        .get(format!("{}/events", app.base_url));
    if let Some(id) = last_event_id {
        req = req.header("Last-Event-ID", id);
    }
    Ok(FrameStream::new(req.send().await?.error_for_status()?))
}

async fn recv_frame(stream: &mut FrameStream, deadline: Duration) -> Result<Frame> {
    timeout(deadline, stream.next_frame())
        .await
        .map_err(|_| anyhow!("timed out waiting for SSE frame"))?
        .and_then(|f| f.ok_or_else(|| anyhow!("stream ended")))
}

// ---------------------------------------------------------------------------
// Contract 1: fresh connect → snapshot first
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fresh_connect_emits_snapshot_first() -> Result<()> {
    let app = TestApp::spawn().await?;
    let mut stream = open_stream(&app, None).await?;
    let frame = recv_frame(&mut stream, Duration::from_secs(3)).await?;
    assert_eq!(frame.event_tag().as_deref(), Some("snapshot"));
    // Initial snapshot carries empty registries in a fresh harness.
    let value = frame.json().expect("json payload");
    assert!(value["jobs"].as_array().is_some_and(|a| a.is_empty()));
    assert!(value["downloads"].as_array().is_some_and(|a| a.is_empty()));
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract 2: bus-published events reach live subscribers with monotonic ids
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_events_include_monotonic_ids() -> Result<()> {
    let app = TestApp::spawn().await?;
    let mut stream = open_stream(&app, None).await?;
    // Drain the initial snapshot.
    let snap = recv_frame(&mut stream, Duration::from_secs(2)).await?;
    assert_eq!(snap.event_tag().as_deref(), Some("snapshot"));

    // Publish directly into the bus so the test doesn't depend on real pipelines.
    let seq_a = app.app.bus.publish(job_started("a"));
    let seq_b = app.app.bus.publish(job_finished("a"));

    let fa = recv_frame(&mut stream, Duration::from_secs(2)).await?;
    let fb = recv_frame(&mut stream, Duration::from_secs(2)).await?;
    assert_eq!(fa.id.as_deref(), Some(seq_a.to_string()).as_deref());
    assert_eq!(fb.id.as_deref(), Some(seq_b.to_string()).as_deref());
    assert_eq!(fa.event_tag().as_deref(), Some("jobStarted"));
    assert_eq!(fb.event_tag().as_deref(), Some("jobFinished"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract 3: reconnect with Last-Event-ID replays missed events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reconnect_with_last_event_id_replays_missed_events() -> Result<()> {
    let app = TestApp::spawn().await?;
    // Publish before the client ever connects.
    let seq_a = app.app.bus.publish(job_started("a"));
    let _seq_b = app.app.bus.publish(job_finished("a"));
    let seq_c = app.app.bus.publish(job_started("b"));

    // Resume from just after event `a` — we expect b + c to arrive, in order.
    let mut stream = open_stream(&app, Some(&seq_a.to_string())).await?;

    let f1 = recv_frame(&mut stream, Duration::from_secs(2)).await?;
    let f2 = recv_frame(&mut stream, Duration::from_secs(2)).await?;
    let ids: Vec<_> = [f1.id.as_deref(), f2.id.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(ids[0].parse::<u64>()?, seq_a + 1);
    assert_eq!(ids[1].parse::<u64>()?, seq_c);
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract 4: Last-Event-ID beyond the ring window falls back to snapshot
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reconnect_from_evicted_id_gets_snapshot() -> Result<()> {
    let app = TestApp::spawn().await?;
    // Publish far past the ring's capacity so earlier entries are evicted.
    // `App::EVENT_BUS_CAPACITY` is 256; we publish 260 to force eviction.
    for i in 0..260 {
        app.app.bus.publish(job_started(&format!("j{i}")));
    }

    // Resume from id 0 — it's been evicted, so the server re-seeds with
    // snapshot + replays the tail of the ring.
    let mut stream = open_stream(&app, Some("0")).await?;
    let first = recv_frame(&mut stream, Duration::from_secs(2)).await?;
    assert_eq!(first.event_tag().as_deref(), Some("snapshot"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract 5: Last-Event-ID pointing at latest receives nothing extra
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reconnect_at_head_just_tails_live() -> Result<()> {
    let app = TestApp::spawn().await?;
    let seq = app.app.bus.publish(job_started("a"));

    let mut stream = open_stream(&app, Some(&seq.to_string())).await?;
    // No replay (we supplied the latest id). The next frame should only
    // arrive once we publish something new.
    let frame_before_publish = timeout(Duration::from_millis(250), stream.next_frame())
        .await
        .ok();
    assert!(
        frame_before_publish.is_none(),
        "should have no replay when caller already has the latest id"
    );

    let seq_b = app.app.bus.publish(job_finished("a"));
    let frame = recv_frame(&mut stream, Duration::from_secs(2)).await?;
    assert_eq!(frame.id.as_deref(), Some(seq_b.to_string()).as_deref());
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract 6: multiple concurrent subscribers each see every event
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_subscribers_both_see_events() -> Result<()> {
    let app = TestApp::spawn().await?;
    let mut a = open_stream(&app, None).await?;
    let mut b = open_stream(&app, None).await?;
    // Drain snapshots.
    recv_frame(&mut a, Duration::from_secs(2)).await?;
    recv_frame(&mut b, Duration::from_secs(2)).await?;

    let seq = app.app.bus.publish(job_started("a"));
    let fa = recv_frame(&mut a, Duration::from_secs(2)).await?;
    let fb = recv_frame(&mut b, Duration::from_secs(2)).await?;
    assert_eq!(fa.id.as_deref(), Some(seq.to_string()).as_deref());
    assert_eq!(fb.id.as_deref(), Some(seq.to_string()).as_deref());
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract 7: end-to-end through a real POST /pipelines
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pipeline_run_surfaces_job_lifecycle_events() -> Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("ev").await?;

    // Subscribe first so we don't race the job events.
    let mut stream = open_stream(&app, None).await?;
    let _ = recv_frame(&mut stream, Duration::from_secs(2)).await?; // snapshot

    // Kick a pipeline. We don't need a real model: `koharu-renderer` is
    // cheap and requires no pages, so the run reaches JobFinished quickly.
    let form = serde_json::json!({
        "steps": ["koharu-renderer"],
        "pages": [],
    });
    app.client_config
        .client
        .post(format!("{}/pipelines", app.base_url))
        .json(&form)
        .send()
        .await?
        .error_for_status()?;

    // Collect frames until we see JobStarted and JobFinished.
    let mut saw_started = false;
    let mut saw_finished = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline && !(saw_started && saw_finished) {
        let Ok(frame) = recv_frame(&mut stream, Duration::from_secs(3)).await else {
            break;
        };
        match frame.event_tag().as_deref() {
            Some("jobStarted") => saw_started = true,
            Some("jobFinished") => saw_finished = true,
            _ => {}
        }
    }
    assert!(saw_started, "expected jobStarted in SSE stream");
    assert!(saw_finished, "expected jobFinished in SSE stream");
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract 8: scene mutations do NOT broadcast (regression guard)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn page_import_does_not_broadcast() -> Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("noop").await?;
    let mut stream = open_stream(&app, None).await?;
    let _ = recv_frame(&mut stream, Duration::from_secs(2)).await?; // snapshot

    let form = Form::new().part(
        "file",
        Part::bytes(TestApp::tiny_png(8, 8, [0, 0, 0, 255]))
            .file_name("a.png".to_string())
            .mime_str("image/png")?,
    );
    let client = app.client_config.client.clone();
    let url = format!("{}/pages", app.base_url);
    tokio::spawn(async move {
        let _ = client.post(url).multipart(form).send().await;
    });

    // Nothing op-shaped should arrive; only the JobStarted/Finished for
    // pipelines would (there are none here). Poll briefly and assert that
    // no `opApplied`/`projectOpened` etc. frames slip through.
    for _ in 0..3 {
        if let Ok(frame) = recv_frame(&mut stream, Duration::from_millis(500)).await {
            let tag = frame.event_tag();
            assert_ne!(tag.as_deref(), Some("opApplied"));
            assert_ne!(tag.as_deref(), Some("opUndone"));
            assert_ne!(tag.as_deref(), Some("projectOpened"));
            assert_ne!(tag.as_deref(), Some("projectClosed"));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract 9: LLM lifecycle events broadcast
// ---------------------------------------------------------------------------

#[tokio::test]
async fn llm_unload_emits_event() -> Result<()> {
    let app = TestApp::spawn().await?;
    let mut stream = open_stream(&app, None).await?;
    // Drain snapshot.
    let _ = recv_frame(&mut stream, Duration::from_secs(2)).await?;

    // Publish directly so the test doesn't need a real model loaded.
    app.app.bus.publish(llm_unloaded());

    let frame = recv_frame(&mut stream, Duration::from_secs(2)).await?;
    assert_eq!(frame.event_tag().as_deref(), Some("llmUnloaded"));
    Ok(())
}

#[tokio::test]
async fn delete_llm_route_publishes_unloaded_event() -> Result<()> {
    let app = TestApp::spawn().await?;
    let mut stream = open_stream(&app, None).await?;
    let _ = recv_frame(&mut stream, Duration::from_secs(2)).await?;

    // DELETE /llm/current — no model is loaded in tests, but the route
    // still publishes the event unconditionally (matches the contract).
    app.client_config
        .client
        .delete(format!("{}/llm/current", app.base_url))
        .send()
        .await?
        .error_for_status()?;

    let frame = recv_frame(&mut stream, Duration::from_secs(2)).await?;
    assert_eq!(frame.event_tag().as_deref(), Some("llmUnloaded"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract 10: subscriber count reflects live connections
// ---------------------------------------------------------------------------

#[tokio::test]
async fn subscriber_count_tracks_live_connections() -> Result<()> {
    let app = TestApp::spawn().await?;
    assert_eq!(app.app.bus.subscriber_count(), 0);

    let _s1 = open_stream(&app, None).await?;
    // Give axum time to upgrade + subscribe.
    wait_until(
        || app.app.bus.subscriber_count() >= 1,
        Duration::from_secs(2),
    )
    .await?;

    let _s2 = open_stream(&app, None).await?;
    wait_until(
        || app.app.bus.subscriber_count() >= 2,
        Duration::from_secs(2),
    )
    .await?;
    Ok(())
}

async fn wait_until<F: Fn() -> bool>(cond: F, total: Duration) -> Result<()> {
    let deadline = std::time::Instant::now() + total;
    while std::time::Instant::now() < deadline {
        if cond() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Err(anyhow!("timed out waiting for condition"))
}

// ---------------------------------------------------------------------------
// Unused imports safety net (silences warnings on helpers we shuffle around)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _unused() -> (Arc<()>, Box<dyn futures::Stream<Item = u8> + Unpin>) {
    // Reference the unused futures import so rustc won't complain when
    // tests get reshuffled.
    let s: Box<dyn futures::Stream<Item = u8> + Unpin> = Box::new(futures::stream::empty().boxed());
    (Arc::new(()), s)
}
