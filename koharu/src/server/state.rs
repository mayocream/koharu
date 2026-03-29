use std::sync::Arc;

use tokio::sync::OnceCell;

use crate::services::AppResources;

pub(crate) type SharedResources = Arc<OnceCell<AppResources>>;

pub(crate) fn get_resources(shared: &SharedResources) -> anyhow::Result<AppResources> {
    shared
        .get()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Resources not initialized"))
}
