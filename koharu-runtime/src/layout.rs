use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::Settings;

const DOWNLOADS_DIR: &str = ".downloads";
const HUGGINGFACE_DIR: &str = "huggingface";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    runtime_root: PathBuf,
    models_root: PathBuf,
    downloads_root: PathBuf,
    huggingface_root: PathBuf,
}

impl Layout {
    pub fn from_settings(settings: &Settings) -> Self {
        let runtime_root = settings.runtime_root().to_path_buf();
        let models_root = settings.models_root().to_path_buf();
        let downloads_root = runtime_root.join(DOWNLOADS_DIR);
        let huggingface_root = models_root.join(HUGGINGFACE_DIR);

        Self {
            runtime_root,
            models_root,
            downloads_root,
            huggingface_root,
        }
    }

    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    pub fn models_root(&self) -> &Path {
        &self.models_root
    }

    pub fn downloads_root(&self) -> &Path {
        &self.downloads_root
    }

    pub fn huggingface_root(&self) -> &Path {
        &self.huggingface_root
    }

    pub fn runtime_package_dir(&self, package: &str) -> PathBuf {
        self.runtime_root.join(package)
    }

    pub fn ensure_roots(&self) -> Result<()> {
        ensure_dir(self.runtime_root())?;
        ensure_dir(self.models_root())?;
        ensure_dir(self.downloads_root())?;
        ensure_dir(self.huggingface_root())?;
        Ok(())
    }
}

fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("failed to create `{}`", path.display()))
}
