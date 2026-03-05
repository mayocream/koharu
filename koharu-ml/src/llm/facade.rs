use std::sync::Arc;

use serde::Serialize;
use tokio::sync::RwLock;

use koharu_types::{Document, TextBlock};

use super::{GenerateOptions, Llm, ModelId};

pub use super::prefetch;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub languages: Vec<String>,
    pub source: &'static str,
}

impl ModelInfo {
    pub fn new(id: ModelId) -> Self {
        Self {
            id: id.to_string(),
            languages: id.languages(),
            source: "local",
        }
    }

    pub fn api(provider_id: &'static str, model_id: &str) -> Self {
        Self {
            id: format!("{provider_id}:{model_id}"),
            languages: vec![],
            source: provider_id,
        }
    }
}

/// Load state of the LLM
#[allow(clippy::large_enum_variant)]
pub enum State {
    Empty,
    Loading,
    Ready(Llm),
    ApiReady {
        provider: Box<dyn super::provider::AnyProvider>,
        model: String,
    },
    Failed(String),
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            State::Empty => write!(f, "empty"),
            State::Loading => write!(f, "loading"),
            State::Ready(_) | State::ApiReady { .. } => write!(f, "ready"),
            State::Failed(_) => write!(f, "failed"),
        }
    }
}

/// Minimal owner for the LLM with non-blocking initialization.
pub struct Model {
    state: Arc<RwLock<State>>,
    use_cpu: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self::new(false)
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
    pub fn new(use_cpu: bool) -> Self {
        Self {
            state: Arc::new(RwLock::new(State::Empty)),
            use_cpu,
        }
    }

    pub fn is_cpu(&self) -> bool {
        self.use_cpu
    }

    /// Start loading an API-backed provider and return immediately.
    pub async fn load_api(
        &self,
        provider_id: &str,
        model_id: &str,
        api_key: String,
    ) -> anyhow::Result<()> {
        use super::provider::AnyProvider;
        let provider: Box<dyn AnyProvider> = match provider_id {
            "openai" => Box::new(super::provider::openai::OpenAiProvider { api_key }),
            "gemini" => Box::new(super::provider::gemini::GeminiProvider { api_key }),
            "claude" => Box::new(super::provider::claude::ClaudeProvider { api_key }),
            other => anyhow::bail!("Unknown API provider: {other}"),
        };
        *self.state.write().await = State::ApiReady {
            provider,
            model: model_id.to_string(),
        };
        Ok(())
    }

    /// Start loading the model on a blocking thread and return immediately.
    pub async fn load(&self, id: ModelId) {
        // mark as loading
        {
            let mut guard = self.state.write().await;
            *guard = State::Loading;
        }

        let state_cloned = self.state.clone();
        let use_cpu = self.use_cpu;
        tokio::spawn(async move {
            let res = Llm::load(id, use_cpu).await;
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
    pub async fn get(&self) -> tokio::sync::RwLockReadGuard<'_, State> {
        self.state.read().await
    }

    /// Returns a write guard to the internal state.
    pub async fn get_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, State> {
        self.state.write().await
    }

    /// Drop the loaded model from memory.
    pub async fn offload(&self) {
        *self.state.write().await = State::Empty;
    }

    /// Ready if the model is loaded into memory.
    pub async fn ready(&self) -> bool {
        matches!(
            *self.state.read().await,
            State::Ready(_) | State::ApiReady { .. }
        )
    }

    /// Translate text using the loaded model.
    pub async fn translate(
        &self,
        doc: &mut impl Translatable,
        target_language: Option<&str>,
    ) -> anyhow::Result<()> {
        let lang = target_language.unwrap_or("English");
        let mut guard = self.state.write().await;
        match &mut *guard {
            State::Ready(llm) => {
                let text = doc.get_source()?;
                let response = llm.generate(&text, &GenerateOptions::default(), target_language)?;
                let response = response.trim().to_string();
                doc.set_translation(response)
            }
            State::ApiReady { provider, model } => {
                let text = doc.get_source()?;
                let model = model.clone();
                let response = provider.translate(&text, lang, &model).await?;
                let response = response.trim().to_string();
                doc.set_translation(response)
            }
            State::Loading => Err(anyhow::anyhow!("Model is still loading")),
            State::Failed(e) => Err(anyhow::anyhow!("Model failed to load: {e}")),
            State::Empty => Err(anyhow::anyhow!("No model is loaded")),
        }
    }
}
