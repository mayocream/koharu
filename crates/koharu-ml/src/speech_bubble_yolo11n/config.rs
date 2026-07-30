use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Yolo11nSpeechBubbleConfig {
    pub model_type: String,
    pub variant: String,
    pub input_size: i64,
    pub num_classes: i64,
    pub num_masks: i64,
    pub num_prototypes: i64,
    pub reg_max: i64,
    pub class_names: Vec<String>,
    pub default_confidence_threshold: f32,
    pub default_nms_threshold: f32,
    pub mask_threshold: f32,
    pub letterbox_color: u8,
}

impl Yolo11nSpeechBubbleConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json).with_context(|| format!("failed to parse {}", path.display()))
    }
}
