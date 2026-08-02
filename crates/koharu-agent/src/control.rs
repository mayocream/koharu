use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::Notify;

#[derive(Clone, Debug, Default)]
pub struct Control {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    cancelled: AtomicBool,
    changed: Notify,
}

impl Control {
    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.changed.notify_waiters();
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        loop {
            let notified = self.inner.changed.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }

    pub(crate) fn ensure_running(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.is_cancelled(), "agent run was cancelled");
        Ok(())
    }
}
