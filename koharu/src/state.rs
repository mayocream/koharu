use std::{path::PathBuf, sync::Arc};

use image::GenericImageView;
use koharu_core::image::SerializableDynamicImage;
use koharu_renderer::types::Color;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
    pub text: Option<String>,
    pub translation: Option<String>,
    pub style: TextStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    pub font_families: Vec<String>,
    pub font_size: Option<f32>,
    pub color: Color,
    pub line_height: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            font_families: vec!["Microsoft YaHei".to_string(), "Arial".to_string()],
            font_size: None,
            color: [0, 0, 0, 255],
            line_height: 1.2,
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
}

impl Document {
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        match path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
            .as_str()
        {
            "khr" => Self::khr(path),
            _ => Self::image(path),
        }
    }

    fn image(path: PathBuf) -> anyhow::Result<Self> {
        let bytes = std::fs::read(&path)?;
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

    fn khr(path: PathBuf) -> anyhow::Result<Self> {
        let bytes = std::fs::read(&path)?;
        let doc: Document = postcard::from_bytes(&bytes)?;
        Ok(doc)
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub documents: Vec<Document>,
}

pub type AppState = Arc<RwLock<State>>;
