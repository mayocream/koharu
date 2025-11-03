use std::{path::PathBuf, sync::Arc};

use image::GenericImageView;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::image::SerializableDynamicImage;

/// A block of text detected in an image.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub confidence: f32,
    pub text: Option<String>,
    pub translation: Option<String>,
}

/// Represents a document with associated metadata and images.
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

    /// Load a document from an image file.
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
            text_blocks: Vec::new(),
            segment: None,
            inpainted: None,
        })
    }

    /// Load a document from a Koharu (.khr) file.
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
