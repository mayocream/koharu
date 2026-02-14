pub mod operations;
pub mod pipeline;

use std::sync::Arc;

use koharu_ml::DeviceName;
use koharu_renderer::facade::Renderer;
use koharu_types::AppState;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppResources {
    pub state: AppState,
    pub ml: Arc<koharu_ml::facade::Model>,
    pub llm: Arc<koharu_ml::llm::facade::Model>,
    pub renderer: Arc<Renderer>,
    pub ml_device: DeviceName,
    pub pipeline: Arc<RwLock<Option<pipeline::PipelineHandle>>>,
    pub version: &'static str,
}
