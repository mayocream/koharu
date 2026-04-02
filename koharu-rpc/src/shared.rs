use std::sync::Arc;

use koharu_app::AppResources;
use koharu_runtime::RuntimeManager;
use tokio::sync::OnceCell;

#[derive(Clone)]
pub struct SharedState {
    inner: Arc<Inner>,
}

struct Inner {
    resources: Arc<OnceCell<AppResources>>,
    runtime: RuntimeManager,
}

impl SharedState {
    pub fn new(resources: Arc<OnceCell<AppResources>>, runtime: RuntimeManager) -> Self {
        Self {
            inner: Arc::new(Inner { resources, runtime }),
        }
    }

    pub fn get(&self) -> Option<AppResources> {
        self.inner.resources.get().cloned()
    }

    pub fn runtime(&self) -> RuntimeManager {
        self.inner.runtime.clone()
    }
}
