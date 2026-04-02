use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::Settings;

const DOWNLOADS_DIR: &str = ".downloads";
const HUGGINGFACE_DIR: &str = "huggingface";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub(crate) runtime_root: PathBuf,
    pub(crate) models_root: PathBuf,
    pub(crate) downloads_root: PathBuf,
    pub(crate) huggingface_root: PathBuf,
}

impl Layout {
    pub fn from_settings(settings: &Settings) -> Self {
        let runtime_root = settings.runtime.path.clone().into_std_path_buf();
        let models_root = settings.models.path.clone().into_std_path_buf();
        let downloads_root = runtime_root.join(DOWNLOADS_DIR);
        let huggingface_root = models_root.join(HUGGINGFACE_DIR);

        Self {
            runtime_root,
            models_root,
            downloads_root,
            huggingface_root,
        }
    }

    pub fn ensure_roots(&self) -> Result<()> {
        fs::create_dir_all(&self.runtime_root)
            .with_context(|| format!("failed to create `{}`", self.runtime_root.display()))?;
        fs::create_dir_all(&self.models_root)
            .with_context(|| format!("failed to create `{}`", self.models_root.display()))?;
        fs::create_dir_all(&self.downloads_root)
            .with_context(|| format!("failed to create `{}`", self.downloads_root.display()))?;
        fs::create_dir_all(&self.huggingface_root)
            .with_context(|| format!("failed to create `{}`", self.huggingface_root.display()))?;
        Ok(())
    }
}
