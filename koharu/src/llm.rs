use koharu_ml::llm::{GenerateOptions, Llm, ModelId};
use std::sync::Arc;
use strum::Display;
use tokio::sync::RwLock;

use crate::state::{Document, TextBlock};

pub use koharu_ml::llm::prefetch;

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

pub trait Translatable {
    fn get_source(&self) -> anyhow::Result<String>;
    fn set_translation(&mut self, translation: String) -> anyhow::Result<()>;
}

impl Translatable for Document {
    fn get_source(&self) -> anyhow::Result<String> {
        let source = self
            .text_blocks
            .clone()
            .into_iter()
            .map(|block| block.text.unwrap_or_else(|| "<empty>".to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(source)
    }

    fn set_translation(&mut self, translation: String) -> anyhow::Result<()> {
        let translations = translation.split("\n").collect::<Vec<_>>();
        for (block, translation) in self.text_blocks.iter_mut().zip(translations) {
            block.translation = Some(translation.to_string());
        }
        Ok(())
    }
}

impl Translatable for TextBlock {
    fn get_source(&self) -> anyhow::Result<String> {
        let source = self
            .text
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No source text found"))?;
        Ok(source)
    }

    fn set_translation(&mut self, translation: String) -> anyhow::Result<()> {
        self.translation = Some(translation);
        Ok(())
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
            let res = Llm::load(id).await;
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
    pub async fn generate(&self, doc: &mut impl Translatable) -> anyhow::Result<()> {
        let mut guard = self.state.write().await;
        match &mut *guard {
            State::Ready(llm) => {
                let text = doc.get_source()?;
                let response = llm.generate(&text, &GenerateOptions::default())?;
                let response = response.trim().to_string();
                doc.set_translation(response)
            }
            State::Loading => Err(anyhow::anyhow!("Model is still loading")),
            State::Failed(e) => Err(anyhow::anyhow!("Model failed to load: {}", e)),
            State::Empty => Err(anyhow::anyhow!("No model is loaded")),
        }
    }
}
