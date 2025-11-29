use koharu_ml::llm::{GenerateOptions, Llm, ModelId};
use std::sync::Arc;
use strum::Display;
use tokio::sync::RwLock;

use crate::state::Document;

/// Load state of the LLM
#[allow(clippy::large_enum_variant)]
#[derive(Display)]
#[strum(serialize_all = "lowercase")]
pub enum State {
    Empty,
    Loading,
    #[strum(serialize = "ready")]
    Ready(Llm),
    Failed(String),
}

/// Minimal owner for the LLM with non-blocking initialization.
pub struct Model {
    state: Arc<RwLock<State>>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
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
            let res = Llm::new(id).await;
            match res {
                Ok(llm) => {
                    let mut guard = state_cloned.write().await;
                    *guard = State::Ready(llm);
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

    /// Returns a write guard to the internal state.
    /// Needed for operations that mutate the loaded LLM instance.
    pub async fn get_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, State> {
        self.state.write().await
    }

    /// Drop the loaded model from memory.
    pub async fn offload(&self) {
        *self.state.write().await = State::Empty;
    }

    /// Ready if the model is loaded into memory.
    pub async fn ready(&self) -> bool {
        matches!(*self.state.read().await, State::Ready(_))
    }

    /// Generate text from the loaded model.
    pub async fn generate(&self, doc: &mut Document) -> anyhow::Result<()> {
        let mut guard = self.state.write().await;
        match &mut *guard {
            State::Ready(llm) => {
                let text = doc
                    .text_blocks
                    .clone()
                    .into_iter()
                    .map(|block| block.text.unwrap_or_else(|| "<empty>".to_string()))
                    .collect::<Vec<_>>()
                    .join("\n");

                let prompt = llm.prompt(text);

                tracing::info!("Generating translation with messages: {:?}", prompt);

                let response = llm.generate(&prompt, &GenerateOptions::default())?;
                let translations = response.split("\n").collect::<Vec<_>>();
                for (block, translation) in doc.text_blocks.iter_mut().zip(translations) {
                    block.translation = Some(translation.to_string());
                }

                Ok(())
            }
            State::Loading => Err(anyhow::anyhow!("Model is still loading")),
            State::Failed(e) => Err(anyhow::anyhow!("Model failed to load: {}", e)),
            State::Empty => Err(anyhow::anyhow!("No model is loaded")),
        }
    }
}
