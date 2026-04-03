pub mod config;
pub mod edit;
pub mod engine;
pub mod io;
pub mod llm;
pub mod pipeline;
pub mod renderer;
pub mod storage;
pub mod utils;

use std::sync::Arc;

use koharu_ml::Device;
use koharu_runtime::RuntimeManager;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::engine::Registry;
use crate::storage::Storage;

#[derive(Clone)]
pub struct AppResources {
    pub runtime: RuntimeManager,
    pub storage: Arc<Storage>,
    pub registry: Arc<Registry>,
    pub config: Arc<RwLock<AppConfig>>,
    pub llm: Arc<llm::Model>,
    pub device: Device,
    pub pipeline: Arc<RwLock<Option<pipeline::PipelineHandle>>>,
    pub version: &'static str,
}
