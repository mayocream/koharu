pub mod config;
pub mod llm;
pub mod ml;
pub mod operations;
pub mod ops;
pub mod pipeline;
pub mod renderer;
pub mod state_tx;

use std::sync::Arc;

use koharu_core::AppState;
use koharu_ml::Device;
use koharu_runtime::RuntimeManager;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppResources {
    pub runtime: RuntimeManager,
    pub state: AppState,
    pub ml: Arc<ml::Model>,
    pub llm: Arc<llm::Model>,
    pub renderer: Arc<renderer::Renderer>,
    pub device: Device,
    pub pipeline: Arc<RwLock<Option<pipeline::PipelineHandle>>>,
    pub version: &'static str,
}
