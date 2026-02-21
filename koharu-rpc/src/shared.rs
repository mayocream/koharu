use std::sync::Arc;

use koharu_pipeline::AppResources;
use tokio::sync::OnceCell;

pub type SharedResources = Arc<OnceCell<AppResources>>;

pub fn get_resources(shared: &SharedResources) -> anyhow::Result<AppResources> {
    shared
        .get()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Resources not initialized"))
}
