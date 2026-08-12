use std::sync::Arc;

use koharu_protocol::{AppEvent, ServerEvent};
use parking_lot::Mutex;
use tokio::sync::broadcast;

const DEFAULT_EVENT_CAPACITY: usize = 256;

/// Ordered live application events. The desktop bridge subscribes before
/// application initialization, so retaining an unused reconnect history would
/// only duplicate the frontend's startup reconciliation.
#[derive(Clone)]
pub struct EventHub {
    next_sequence: Arc<Mutex<u64>>,
    live: broadcast::Sender<ServerEvent>,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new(DEFAULT_EVENT_CAPACITY)
    }
}

impl EventHub {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "an event channel must have positive capacity");
        let (live, _) = broadcast::channel(capacity);
        Self {
            next_sequence: Arc::new(Mutex::new(1)),
            live,
        }
    }

    pub fn publish(&self, event: AppEvent) -> ServerEvent {
        // Sequence assignment and channel publication are one critical section:
        // concurrent application tasks must observe the same order on the wire.
        let mut next_sequence = self.next_sequence.lock();
        let sequence = *next_sequence;
        *next_sequence = sequence.checked_add(1).expect("event sequence exhausted");
        let published = ServerEvent { sequence, event };
        let _ = self.live.send(published.clone());
        published
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.live.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use koharu_protocol::{AppError, AppErrorCode};

    use super::*;

    fn event(message: &str) -> AppEvent {
        AppEvent::StartupFailed {
            error: AppError::new(AppErrorCode::Internal, message),
        }
    }

    #[tokio::test]
    async fn live_events_are_sequenced_from_one() {
        let hub = EventHub::new(4);
        let mut subscription = hub.subscribe();
        assert_eq!(hub.publish(event("one")).sequence, 1);
        assert_eq!(hub.publish(event("two")).sequence, 2);
        assert_eq!(subscription.recv().await.unwrap().sequence, 1);
        assert_eq!(subscription.recv().await.unwrap().sequence, 2);
    }

    #[tokio::test]
    async fn lag_is_reported_by_the_channel() {
        let hub = EventHub::new(2);
        let mut subscription = hub.subscribe();
        hub.publish(event("one"));
        hub.publish(event("two"));
        hub.publish(event("three"));
        assert!(matches!(
            subscription.recv().await,
            Err(broadcast::error::RecvError::Lagged(1))
        ));
    }

    #[test]
    fn concurrent_publishers_deliver_in_sequence_order() {
        let hub = EventHub::new(64);
        let mut subscription = hub.subscribe();
        std::thread::scope(|scope| {
            for publisher in 0..32 {
                let hub = hub.clone();
                scope.spawn(move || {
                    hub.publish(event(&publisher.to_string()));
                });
            }
        });

        let sequences = (0..32)
            .map(|_| subscription.try_recv().unwrap().sequence)
            .collect::<Vec<_>>();
        assert_eq!(sequences, (1..=32).collect::<Vec<_>>());
    }
}
