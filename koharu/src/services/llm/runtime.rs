use std::path::{Path, PathBuf};
use std::sync::Arc;

use koharu_core::{LlmState, LlmStateStatus};
use koharu_llm::providers::{AnyProvider, Provider, ProviderConfig, build_provider};
use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_llm::{GenerateOptions, Language, Llm, ModelId};
use tokio::sync::{Mutex, RwLock, broadcast};

use super::translation::TranslationTarget;

#[derive(Clone)]
struct LocalSession {
    id: ModelId,
    llm: Arc<Mutex<Llm>>,
}

#[derive(Clone)]
struct ApiSession {
    provider: Arc<Provider>,
    provider_id: String,
    model_id: String,
}

#[derive(Clone)]
enum LoadedModel {
    Local(LocalSession),
    Api(ApiSession),
}

#[derive(Clone)]
enum RuntimeState {
    Empty,
    Loading { model_id: String, source: String },
    Ready(LoadedModel),
    Failed(String),
}

pub(crate) struct LlmRuntime {
    state: Arc<RwLock<RuntimeState>>,
    state_tx: broadcast::Sender<LlmState>,
    cpu: bool,
    backend: Arc<LlamaBackend>,
    runtime_root: PathBuf,
    models_root: PathBuf,
}

impl LlmRuntime {
    pub(crate) fn new(
        cpu: bool,
        backend: Arc<LlamaBackend>,
        runtime_root: &Path,
        models_root: &Path,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(RuntimeState::Empty)),
            state_tx: broadcast::channel(64).0,
            cpu,
            backend,
            runtime_root: runtime_root.to_path_buf(),
            models_root: models_root.to_path_buf(),
        }
    }

    pub(crate) fn is_cpu(&self) -> bool {
        self.cpu
    }

    pub(crate) async fn load_api(
        &self,
        provider_id: &str,
        model_id: &str,
        config: ProviderConfig,
    ) -> anyhow::Result<()> {
        let provider = Arc::new(build_provider(provider_id, config)?);
        *self.state.write().await = RuntimeState::Ready(LoadedModel::Api(ApiSession {
            provider,
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
        }));
        self.emit_state().await;
        Ok(())
    }

    pub(crate) async fn load_local(&self, id: ModelId) {
        {
            let mut guard = self.state.write().await;
            *guard = RuntimeState::Loading {
                model_id: id.to_string(),
                source: "local".to_string(),
            };
        }
        self.emit_state().await;

        let state = Arc::clone(&self.state);
        let state_tx = self.state_tx.clone();
        let backend = Arc::clone(&self.backend);
        let runtime_root = self.runtime_root.clone();
        let models_root = self.models_root.clone();
        let cpu = self.cpu;

        tokio::spawn(async move {
            let result = Llm::load(id, cpu, backend, &runtime_root, &models_root).await;
            let next_state = match result {
                Ok(llm) => RuntimeState::Ready(LoadedModel::Local(LocalSession {
                    id,
                    llm: Arc::new(Mutex::new(llm)),
                })),
                Err(err) => {
                    tracing::error!(%err, model_id = %id, "LLM load failed");
                    RuntimeState::Failed(err.to_string())
                }
            };

            let snapshot = {
                let mut guard = state.write().await;
                *guard = next_state;
                snapshot_from_state(&guard)
            };
            let _ = state_tx.send(snapshot);
        });
    }

    pub(crate) async fn offload(&self) {
        *self.state.write().await = RuntimeState::Empty;
        self.emit_state().await;
    }

    pub(crate) async fn ready(&self) -> bool {
        matches!(&*self.state.read().await, RuntimeState::Ready(_))
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<LlmState> {
        self.state_tx.subscribe()
    }

    pub(crate) async fn snapshot(&self) -> LlmState {
        let guard = self.state.read().await;
        snapshot_from_state(&guard)
    }

    pub(crate) async fn translate(
        &self,
        target: &mut impl TranslationTarget,
        target_language: Option<&str>,
    ) -> anyhow::Result<()> {
        let target_language = target_language
            .and_then(Language::parse)
            .unwrap_or(Language::English);
        let source = target.translation_source()?;
        if source.is_empty() {
            tracing::debug!("skipping translate: no source text");
            return Ok(());
        }

        let loaded = {
            let guard = self.state.read().await;
            match &*guard {
                RuntimeState::Ready(model) => Ok(model.clone()),
                RuntimeState::Loading { .. } => Err(anyhow::anyhow!("Model is still loading")),
                RuntimeState::Failed(error) => {
                    Err(anyhow::anyhow!("Model failed to load: {error}"))
                }
                RuntimeState::Empty => Err(anyhow::anyhow!("No model is loaded")),
            }
        }?;

        let translation = match loaded {
            LoadedModel::Local(session) => {
                let mut llm = session.llm.lock().await;
                llm.generate(&source, &GenerateOptions::default(), target_language)
            }
            LoadedModel::Api(session) => {
                session
                    .provider
                    .translate(&source, target_language, &session.model_id)
                    .await
            }
        }?;

        target.apply_translation(translation.trim().to_string())
    }

    async fn emit_state(&self) {
        let _ = self.state_tx.send(self.snapshot().await);
    }
}

fn snapshot_from_state(state: &RuntimeState) -> LlmState {
    match state {
        RuntimeState::Empty => LlmState {
            status: LlmStateStatus::Empty,
            model_id: None,
            source: None,
            error: None,
        },
        RuntimeState::Loading { model_id, source } => LlmState {
            status: LlmStateStatus::Loading,
            model_id: Some(model_id.clone()),
            source: Some(source.clone()),
            error: None,
        },
        RuntimeState::Ready(LoadedModel::Local(session)) => LlmState {
            status: LlmStateStatus::Ready,
            model_id: Some(session.id.to_string()),
            source: Some("local".to_string()),
            error: None,
        },
        RuntimeState::Ready(LoadedModel::Api(session)) => LlmState {
            status: LlmStateStatus::Ready,
            model_id: Some(format!("{}:{}", session.provider_id, session.model_id)),
            source: Some(session.provider_id.clone()),
            error: None,
        },
        RuntimeState::Failed(error) => LlmState {
            status: LlmStateStatus::Failed,
            model_id: None,
            source: None,
            error: Some(error.clone()),
        },
    }
}
