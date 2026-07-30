use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const CAPACITY: usize = 256;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static EVENTS: LazyLock<broadcast::Sender<Event>> =
    LazyLock::new(|| broadcast::channel(CAPACITY).0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Event {
    Started {
        id: u64,
        name: String,
    },
    Progress {
        id: u64,
        name: String,
        completed: u64,
        total: u64,
    },
    Finished {
        id: u64,
    },
    Failed {
        id: u64,
        name: String,
        error: String,
    },
}

#[must_use]
pub fn subscribe() -> broadcast::Receiver<Event> {
    EVENTS.subscribe()
}

pub(super) fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub(super) fn publish(event: Event) {
    let _ = EVENTS.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribers_receive_download_events() {
        let mut events = subscribe();
        let id = next_id();
        let event = Event::Started {
            id,
            name: "model.bin".into(),
        };

        publish(event.clone());

        assert_eq!(events.try_recv().unwrap(), event);
    }
}
