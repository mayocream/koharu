pub(crate) mod documents;
pub(crate) mod editing;
pub(crate) mod llm;
pub(crate) mod pipeline;
pub(crate) mod rendering;
pub(crate) mod request;
pub(crate) mod store;
pub(crate) mod support;
pub(crate) mod vision;

pub(crate) mod operations {
    pub(crate) use super::documents::*;
    pub(crate) use super::editing::*;
    pub(crate) use super::llm::*;
    pub(crate) use super::pipeline::jobs::*;
    pub(crate) use super::rendering::*;
    pub(crate) use super::vision::*;
}

use std::sync::Arc;

use koharu_core::AppState;
use koharu_ml::Device;
use tokio::sync::RwLock;

#[derive(Clone)]
pub(crate) struct AppResources {
    pub(crate) state: AppState,
    pub(crate) vision: Arc<vision::VisionRuntime>,
    pub(crate) llm: Arc<llm::LlmRuntime>,
    pub(crate) renderer: Arc<rendering::RendererRuntime>,
    pub(crate) device: Device,
    pub(crate) pipeline: Arc<RwLock<Option<pipeline::runner::PipelineHandle>>>,
    pub(crate) version: &'static str,
}
