use std::{path::PathBuf, sync::Arc};

use anyhow::anyhow;
use image::GenericImageView;
use koharu_ml::font_detector::FontPrediction;
use koharu_renderer::renderer::TextShaderEffect;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{
    image::SerializableDynamicImage,
    khr::{deserialize_khr, has_khr_magic},
};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextBlock {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
    pub text: Option<String>,
    pub translation: Option<String>,
    pub style: Option<TextStyle>,
    pub font_prediction: Option<FontPrediction>,
    pub rendered: Option<SerializableDynamicImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    pub font_families: Vec<String>,
    pub font_size: Option<f32>,
    pub color: [u8; 4],
    pub effect: Option<TextShaderEffect>,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            font_families: vec![
                // Windows defaults
                "Microsoft YaHei".to_string(),
                "Microsoft Jhenghei".to_string(),
                "Yu Mincho".to_string(),
                // macOS defaults
                "PingFang TC".to_string(),
                "PingFang SC".to_string(),
                "Hiragino Mincho".to_string(),
                "SF Pro".to_string(),
                // linux defaults
                "Source Han Sans CN".to_string(),
                // Fallback
                "Arial".to_string(),
            ],
            font_size: None,
            color: [0, 0, 0, 255],
            effect: None,
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub image: SerializableDynamicImage,
    pub width: u32,
    pub height: u32,
    pub text_blocks: Vec<TextBlock>,
    pub segment: Option<SerializableDynamicImage>,
    pub inpainted: Option<SerializableDynamicImage>,
    pub rendered: Option<SerializableDynamicImage>,
    pub brush_layer: Option<SerializableDynamicImage>,
}

impl Document {
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        let bytes = std::fs::read(&path)?;

        if has_khr_magic(&bytes) {
            return Self::khr(path, bytes);
        }

        Self::image(path, bytes)
    }

    fn image(path: PathBuf, bytes: Vec<u8>) -> anyhow::Result<Self> {
        let img = image::load_from_memory(&bytes)?;
        let (width, height) = img.dimensions();
        let id = blake3::hash(&bytes).to_hex().to_string();
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        Ok(Document {
            id,
            path,
            name,
            image: SerializableDynamicImage(img),
            width,
            height,
            ..Default::default()
        })
    }

    fn khr(_path: PathBuf, bytes: Vec<u8>) -> anyhow::Result<Self> {
        let docs = deserialize_khr(&bytes)?;
        docs.into_iter()
            .next()
            .ok_or_else(|| anyhow!("No document found in KHR file"))
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub documents: Vec<Document>,
}

pub type AppState = Arc<RwLock<State>>;
