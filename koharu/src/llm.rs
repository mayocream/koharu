use llm::{Llm, ModelId};
use std::sync::Arc;
use strum::Display;
use tokio::sync::RwLock;

/// Load state of the LLM
#[derive(Clone, Display)]
#[strum(serialize_all = "lowercase")]
pub enum State {
    Empty,
    Loading,
    #[strum(serialize = "ready")]
    Ready(Arc<Llm>),
    Failed(String),
}

/// Minimal owner for the LLM with non-blocking initialization.
pub struct Model {
    state: Arc<RwLock<State>>,
}

impl Model {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(State::Empty)),
        }
    }

    /// Start loading the model on a blocking thread and return immediately.
    pub async fn load(&self, id: ModelId) {
        // mark as loading
        {
            let mut guard = self.state.write().await;
            *guard = State::Loading;
        }

        let state_cloned = self.state.clone();
        tokio::spawn(async move {
            let res = tokio::task::spawn_blocking(move || Llm::from_pretrained(id)).await;
            match res {
                Ok(Ok(llm)) => {
                    let mut guard = state_cloned.write().await;
                    *guard = State::Ready(Arc::new(llm));
                }
                Ok(Err(e)) => {
                    tracing::error!("LLM load error: {e}");
                    let mut guard = state_cloned.write().await;
                    *guard = State::Failed(e.to_string());
                }
                Err(e) => {
                    tracing::error!("LLM load join error: {e}");
                    let mut guard = state_cloned.write().await;
                    *guard = State::Failed(format!("join error: {e}"));
                }
            }
        });
    }

    /// Returns a read guard to the internal state.
    /// Callers can inspect `State` directly while holding the guard.
    pub async fn get(&self) -> tokio::sync::RwLockReadGuard<'_, State> {
        self.state.read().await
    }

    /// Drop the loaded model from memory.
    pub async fn offload(&self) {
        *self.state.write().await = State::Empty;
    }

    /// Ready if the model is loaded into memory.
    pub async fn ready(&self) -> bool {
        matches!(*self.state.read().await, State::Ready(_))
    }
}
