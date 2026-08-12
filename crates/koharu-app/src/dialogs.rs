use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct DialogFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[async_trait]
pub trait FileDialogs: Send + Sync {
    async fn pick_files(&self, filters: &[DialogFilter]) -> Result<Option<Vec<PathBuf>>>;
    async fn pick_folder(&self) -> Result<Option<PathBuf>>;
    async fn save_file(
        &self,
        suggested_name: &str,
        filters: &[DialogFilter],
    ) -> Result<Option<PathBuf>>;
}
