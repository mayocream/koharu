use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use koharu_app::AppResources;
use koharu_core::BootstrapConfig;
use koharu_runtime::RuntimeManager;
use tokio::sync::{OnceCell, watch};

pub type InitializeFuture = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>>;

#[derive(Clone)]
pub struct BootstrapHooks {
    pub get_config: Arc<dyn Fn() -> anyhow::Result<BootstrapConfig> + Send + Sync>,
    pub put_config: Arc<dyn Fn(BootstrapConfig) -> anyhow::Result<BootstrapConfig> + Send + Sync>,
    pub initialize: Arc<dyn Fn() -> InitializeFuture + Send + Sync>,
}

#[derive(Clone)]
pub struct SharedState {
    inner: Arc<Inner>,
}

struct Inner {
    resources: Arc<OnceCell<AppResources>>,
    runtime: watch::Receiver<RuntimeManager>,
    bootstrap: BootstrapHooks,
}

impl SharedState {
    pub fn new(
        resources: Arc<OnceCell<AppResources>>,
        runtime: watch::Receiver<RuntimeManager>,
        bootstrap: BootstrapHooks,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                resources,
                runtime,
                bootstrap,
            }),
        }
    }

    pub fn get(&self) -> Option<AppResources> {
        self.inner.resources.get().cloned()
    }

    pub fn resources_cell(&self) -> Arc<OnceCell<AppResources>> {
        Arc::clone(&self.inner.resources)
    }

    pub fn runtime(&self) -> RuntimeManager {
        self.inner.runtime.borrow().clone()
    }

    pub fn subscribe_runtime(&self) -> watch::Receiver<RuntimeManager> {
        self.inner.runtime.clone()
    }

    pub async fn get_or_try_init<F, Fut>(&self, init: F) -> anyhow::Result<AppResources>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<AppResources>>,
    {
        Ok(self.inner.resources.get_or_try_init(init).await?.clone())
    }

    pub fn get_config(&self) -> anyhow::Result<BootstrapConfig> {
        (self.inner.bootstrap.get_config)()
    }

    pub fn put_config(&self, config: BootstrapConfig) -> anyhow::Result<BootstrapConfig> {
        (self.inner.bootstrap.put_config)(config)
    }

    pub async fn initialize(&self) -> anyhow::Result<()> {
        (self.inner.bootstrap.initialize)().await
    }
}

pub fn get_resources(shared: &SharedState) -> anyhow::Result<AppResources> {
    shared
        .get()
        .ok_or_else(|| anyhow::anyhow!("Resources not initialized"))
}
