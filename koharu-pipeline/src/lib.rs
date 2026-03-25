pub mod operations;
pub mod ops;
pub mod pipeline;
pub mod state_tx;

use std::sync::Arc;

use koharu_llm::facade;
use koharu_ml::Device;
use koharu_renderer::facade::Renderer;
use koharu_types::AppState;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppResources {
    pub state: AppState,
    pub ml: Arc<koharu_ml::facade::Model>,
    pub llm: Arc<facade::Model>,
    pub renderer: Arc<Renderer>,
    pub device: Device,
    pub pipeline: Arc<RwLock<Option<pipeline::PipelineHandle>>>,
    pub version: &'static str,
}
