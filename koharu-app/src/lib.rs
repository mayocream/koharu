pub mod config;
pub mod edit;
pub mod io;
pub mod llm;
pub mod ml;
pub mod pipeline;
pub mod renderer;
pub mod storage;
pub mod utils;

use std::sync::Arc;

use koharu_ml::Device;
use koharu_runtime::RuntimeManager;
use tokio::sync::RwLock;

use crate::storage::Storage;

#[derive(Clone)]
pub struct AppResources {
    pub runtime: RuntimeManager,
    pub storage: Arc<Storage>,
    pub ml: Arc<ml::Model>,
    pub llm: Arc<llm::Model>,
    pub renderer: Arc<renderer::Renderer>,
    pub device: Device,
    pub pipeline: Arc<RwLock<Option<pipeline::PipelineHandle>>>,
    pub version: &'static str,
}
