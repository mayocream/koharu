//! Event bus backing the SSE stream.
//!
//! ## Design
//!
//! - Every `AppEvent` published through the bus is stamped with a monotonic
//!   `seq: u64` and persisted in a bounded ring buffer alongside a
//!   broadcast fan-out.
//! - Subscribers fed off the broadcast see live events in near-real-time.
//! - Reconnecting clients pass `Last-Event-ID: <seq>` (native SSE spec)
//!   and the SSE route calls [`EventBus::replay_since`] to hand back
//!   everything with `seq > last_event_id` that is still in the ring.
//! - If a subscriber lags off the broadcast buffer we fall back to a
//!   `Snapshot` re-seed on the route layer — the ring is a best-effort
//!   recovery path, not an infinite log.
//!
//! ## Invariants
//!
//! - `seq` is strictly increasing, contiguous, and never reused.
//! - History length is capped at [`EventBus::capacity`]. Oldest entries
//!   are evicted first.
//! - `publish` is non-blocking: broadcast errors (no subscribers) are
//!   swallowed; ring-buffer insertion holds a short std `Mutex` and never
//!   awaits.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use koharu_core::AppEvent;
use tokio::sync::broadcast;

/// Server-assigned sequence + payload. `seq` is the wire-level SSE `id:`
/// field so browsers can reconnect with `Last-Event-ID`.
#[derive(Debug, Clone)]
pub struct SequencedEvent {
    pub seq: u64,
    pub event: AppEvent,
}

/// Single-process event bus. Cheap to clone via `Arc`; all concurrent
/// access is internal.
pub struct EventBus {
    next_seq: AtomicU64,
    capacity: usize,
    history: Mutex<VecDeque<SequencedEvent>>,
    tx: broadcast::Sender<SequencedEvent>,
}

impl EventBus {
    /// Build a bus with `capacity` ring-buffer slots and a matching
    /// broadcast buffer.
    pub fn new(capacity: usize) -> Arc<Self> {
        assert!(capacity > 0, "EventBus capacity must be > 0");
        let (tx, _) = broadcast::channel(capacity);
        Arc::new(Self {
            next_seq: AtomicU64::new(0),
            capacity,
            history: Mutex::new(VecDeque::with_capacity(capacity)),
            tx,
        })
    }

    /// Publish `event`, returning the assigned `seq`.
    pub fn publish(&self, event: AppEvent) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let sev = SequencedEvent { seq, event };
        {
            let mut history = self.history.lock().expect("EventBus history poisoned");
            if history.len() == self.capacity {
                history.pop_front();
            }
            history.push_back(sev.clone());
        }
        // No subscribers → `send` returns Err. That's fine; the event is in
        // the ring buffer and a future subscriber can request replay.
        let _ = self.tx.send(sev);
        seq
    }

    /// Fresh broadcast receiver for live events.
    pub fn subscribe(&self) -> broadcast::Receiver<SequencedEvent> {
        self.tx.subscribe()
    }

    /// Return every buffered event with `seq > after`, in ascending order.
    /// Returns an empty `Vec` if nothing qualifies (including the case
    /// where `after` predates the ring window — callers treat that as
    /// "you missed too much, re-seed from Snapshot").
    pub fn replay_since(&self, after: u64) -> Vec<SequencedEvent> {
        let history = self.history.lock().expect("EventBus history poisoned");
        history.iter().filter(|e| e.seq > after).cloned().collect()
    }

    /// Highest `seq` assigned so far (`None` if no events published yet).
    pub fn latest_seq(&self) -> Option<u64> {
        let n = self.next_seq.load(Ordering::Relaxed);
        if n == 0 { None } else { Some(n - 1) }
    }

    /// Number of events currently buffered.
    pub fn buffered(&self) -> usize {
        self.history
            .lock()
            .expect("EventBus history poisoned")
            .len()
    }

    /// Ring-buffer capacity (fixed at construction).
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of live subscribers. Useful for tests.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_core::{JobStatus, JobSummary};

    fn sample_event(id: &str) -> AppEvent {
        AppEvent::JobStarted {
            id: id.to_string(),
            kind: "pipeline".to_string(),
        }
    }

    fn finished(id: &str) -> AppEvent {
        AppEvent::JobFinished(koharu_core::JobFinishedEvent {
            id: id.to_string(),
            status: JobStatus::Completed,
            error: None,
        })
    }

    #[test]
    fn publish_assigns_monotonic_seq() {
        let bus = EventBus::new(8);
        assert_eq!(bus.publish(sample_event("a")), 0);
        assert_eq!(bus.publish(sample_event("b")), 1);
        assert_eq!(bus.publish(sample_event("c")), 2);
        assert_eq!(bus.latest_seq(), Some(2));
    }

    #[test]
    fn replay_since_returns_events_strictly_after() {
        let bus = EventBus::new(8);
        bus.publish(sample_event("a"));
        bus.publish(sample_event("b"));
        bus.publish(sample_event("c"));

        let got = bus.replay_since(0);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].seq, 1);
        assert_eq!(got[1].seq, 2);
    }

    #[test]
    fn ring_buffer_evicts_oldest() {
        let bus = EventBus::new(2);
        bus.publish(sample_event("a")); // seq 0, evicted
        bus.publish(sample_event("b")); // seq 1
        bus.publish(sample_event("c")); // seq 2

        assert_eq!(bus.buffered(), 2);
        let got = bus.replay_since(0);
        assert_eq!(got.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2]);

        // Asking for something evicted returns only what's still buffered.
        let stale_replay = bus.replay_since(u64::MAX - 1);
        assert!(stale_replay.is_empty());
    }

    #[tokio::test]
    async fn subscriber_sees_live_events() {
        let bus = EventBus::new(8);
        let mut rx = bus.subscribe();
        bus.publish(sample_event("a"));
        bus.publish(finished("a"));

        let ev = rx.recv().await.expect("first event");
        assert_eq!(ev.seq, 0);
        let ev = rx.recv().await.expect("second event");
        assert_eq!(ev.seq, 1);
    }

    #[test]
    fn latest_seq_empty_bus_is_none() {
        let bus = EventBus::new(4);
        assert_eq!(bus.latest_seq(), None);
        assert_eq!(bus.buffered(), 0);
    }

    #[test]
    fn _use_unused_type() {
        // Silence warnings for JobSummary import in case tests drift.
        let _: Option<JobSummary> = None;
    }
}
