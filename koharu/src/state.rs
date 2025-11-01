use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::image::SerializableDynamicImage;

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

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub source: SerializableDynamicImage,
    pub width: u32,
    pub height: u32,
    pub path: PathBuf,
    pub name: String,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub image: Image,
    pub text_blocks: Vec<TextBlock>,
    pub segment: Option<SerializableDynamicImage>,
    pub inpainted: Option<SerializableDynamicImage>,
}
