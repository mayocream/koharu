use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ComicLayoutYolo26sConfig {
    pub architectures: Vec<String>,
    pub library_name: String,
    pub task: String,
    pub image_size: i64,
    pub num_classes: i64,
    pub names: BTreeMap<String, String>,
    pub weights: String,
    pub model_config: String,
    pub ultralytics_version: String,
}

impl ComicLayoutYolo26sConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub(crate) fn class_names(&self) -> Result<Vec<String>> {
        (0..self.num_classes)
            .map(|index| {
                self.names
                    .get(&index.to_string())
                    .cloned()
                    .with_context(|| format!("missing class name for label {index}"))
            })
            .collect()
    }
}
