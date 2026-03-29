use std::sync::Arc;

use arc_swap::ArcSwap;
use koharu_core::Config;
use koharu_runtime::registry::{BootstrapPaths, required_entries};
use thiserror::Error;
use tokio::sync::{Mutex, broadcast};

use crate::bootstrap::config::{BootstrapConfigError, ConfigStore, ProjectPaths};
use crate::services::AppResources;

type ResourceBuilderFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<AppResources>> + Send>>;
type ResourceBuilder = Arc<dyn Fn(BootstrapPaths) -> ResourceBuilderFuture + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapPhase {
    NeedsOnboarding,
    PendingInitialize,
    Initializing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootstrapSnapshot {
    pub(crate) phase: BootstrapPhase,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Error)]
pub(crate) enum BootstrapManagerError {
    #[error(transparent)]
    Config(#[from] BootstrapConfigError),
    #[error("bootstrap entry `{entry_id}` failed: {source}")]
    Dependency {
        entry_id: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to build application resources: {0}")]
    BuildResources(#[source] anyhow::Error),
}

pub(crate) struct BootstrapManager {
    config_store: ConfigStore,
    resources: crate::server::SharedResources,
    builder: ResourceBuilder,
    config: Arc<ArcSwap<Config>>,
    state: Arc<ArcSwap<BootstrapSnapshot>>,
    tx: broadcast::Sender<BootstrapSnapshot>,
    command_lock: Mutex<()>,
}

impl BootstrapManager {
    pub(crate) fn new(
        paths: ProjectPaths,
        resources: crate::server::SharedResources,
        builder: ResourceBuilder,
    ) -> std::result::Result<Arc<Self>, BootstrapManagerError> {
        let config_store = ConfigStore::new(paths);
        let config = config_store.load()?;
        config_store.apply(&config)?;
        let dependency_paths = config_store.dependency_paths(&config)?;
        let phase = if dependencies_ready(&dependency_paths)? {
            BootstrapPhase::PendingInitialize
        } else {
            BootstrapPhase::NeedsOnboarding
        };

        Ok(Arc::new(Self {
            config_store,
            resources,
            builder,
            config: Arc::new(ArcSwap::from_pointee(config)),
            state: Arc::new(ArcSwap::from_pointee(BootstrapSnapshot {
                phase,
                error: None,
            })),
            tx: broadcast::channel(64).0,
            command_lock: Mutex::new(()),
        }))
    }

    pub(crate) fn config(&self) -> Config {
        self.config.load_full().as_ref().clone()
    }

    pub(crate) fn snapshot(&self) -> BootstrapSnapshot {
        self.state.load_full().as_ref().clone()
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<BootstrapSnapshot> {
        self.tx.subscribe()
    }

    pub(crate) async fn update_config(
        &self,
        config: Config,
    ) -> std::result::Result<Config, BootstrapManagerError> {
        let _guard = self.command_lock.lock().await;
        let previous = self.config();
        let current_state = self.snapshot();

        if self.resources.get().is_some() {
            self.config_store.enforce_locked_paths(&previous, &config)?;
        }
        if matches!(current_state.phase, BootstrapPhase::Initializing) {
            self.config_store.enforce_locked_paths(&previous, &config)?;
        }

        self.config_store.apply(&config)?;
        self.config_store.persist(&config)?;
        self.config.store(Arc::new(config.clone()));

        if !matches!(current_state.phase, BootstrapPhase::Initializing) {
            let dependency_paths = self.config_store.dependency_paths(&config)?;
            let phase = if dependencies_ready(&dependency_paths)? {
                if self.resources.get().is_some() {
                    BootstrapPhase::Ready
                } else {
                    BootstrapPhase::PendingInitialize
                }
            } else {
                BootstrapPhase::NeedsOnboarding
            };
            self.replace_state(BootstrapSnapshot { phase, error: None });
        }

        Ok(config)
    }

    pub(crate) async fn initialize(
        self: &Arc<Self>,
    ) -> std::result::Result<(), BootstrapManagerError> {
        let _guard = self.command_lock.lock().await;
        if !matches!(self.snapshot().phase, BootstrapPhase::Initializing) {
            self.replace_state(BootstrapSnapshot {
                phase: BootstrapPhase::Initializing,
                error: None,
            });
        }

        let result = self.run_initialize().await;
        match result {
            Ok(()) => {
                self.replace_state(BootstrapSnapshot {
                    phase: BootstrapPhase::Ready,
                    error: None,
                });
                Ok(())
            }
            Err(error) => {
                self.replace_state(BootstrapSnapshot {
                    phase: BootstrapPhase::Failed,
                    error: Some(error.to_string()),
                });
                Err(error)
            }
        }
    }

    pub(crate) async fn maybe_start_on_launch(
        self: &Arc<Self>,
    ) -> std::result::Result<(), BootstrapManagerError> {
        if matches!(self.snapshot().phase, BootstrapPhase::PendingInitialize) {
            self.replace_state(BootstrapSnapshot {
                phase: BootstrapPhase::Initializing,
                error: None,
            });
            let manager = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(error) = manager.initialize().await {
                    tracing::error!("bootstrap initialization failed: {error:#}");
                }
            });
        }
        Ok(())
    }

    fn replace_state(&self, next: BootstrapSnapshot) {
        self.state.store(Arc::new(next.clone()));
        let _ = self.tx.send(next);
    }

    async fn run_initialize(&self) -> std::result::Result<(), BootstrapManagerError> {
        let config = self.config();
        self.config_store.apply(&config)?;
        let dependency_paths = self.config_store.dependency_paths(&config)?;

        for entry in required_entries() {
            if entry.is_ready(&dependency_paths).map_err(|source| {
                BootstrapManagerError::Dependency {
                    entry_id: entry.id.clone(),
                    source,
                }
            })? {
                continue;
            }

            entry.ensure(&dependency_paths).await.map_err(|source| {
                BootstrapManagerError::Dependency {
                    entry_id: entry.id.clone(),
                    source,
                }
            })?;
        }

        if self.resources.get().is_none() {
            let resources = (self.builder)(dependency_paths)
                .await
                .map_err(BootstrapManagerError::BuildResources)?;
            self.resources.get_or_init(|| async { resources }).await;
        }

        Ok(())
    }
}

fn dependencies_ready(paths: &BootstrapPaths) -> std::result::Result<bool, BootstrapManagerError> {
    for entry in required_entries() {
        if !entry
            .is_ready(paths)
            .map_err(|source| BootstrapManagerError::Dependency {
                entry_id: entry.id.clone(),
                source,
            })?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use koharu_core::{Config, MirrorKind, MirrorSelection};

    fn config() -> Config {
        Config {
            language: "en-US".to_string(),
            runtime_path: "C:/koharu/runtime".to_string(),
            models_path: "C:/koharu/models".to_string(),
            proxy_url: None,
            pypi_mirror: MirrorSelection {
                kind: MirrorKind::Official,
                custom_base_url: None,
            },
            github_mirror: MirrorSelection {
                kind: MirrorKind::Official,
                custom_base_url: None,
            },
        }
    }

    #[test]
    fn language_changes_do_not_change_mirror_identity() {
        let previous = config();
        let mut next = config();
        next.language = "ja-JP".to_string();

        assert_eq!(previous.pypi_mirror.kind, next.pypi_mirror.kind);
        assert_eq!(previous.github_mirror.kind, next.github_mirror.kind);
    }
}
