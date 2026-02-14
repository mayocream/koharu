mod effect;
mod font;
mod image;

pub use effect::TextShaderEffect;
pub use font::{FontPrediction, NamedFontPrediction, TextDirection};
pub use image::SerializableDynamicImage;

use std::{collections::HashSet, path::PathBuf, sync::Arc};

use ::image::GenericImageView;
use serde::{Deserialize, Serialize};
use sys_locale::get_locale;
use tokio::sync::RwLock;

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
            font_families: default_font_families(),
            font_size: None,
            color: [0, 0, 0, 255],
            effect: None,
        }
    }
}

#[derive(Copy, Clone)]
enum FontCategory {
    SimplifiedChinese,
    TraditionalChinese,
    English,
}

fn default_font_families() -> Vec<String> {
    let locale = get_locale().unwrap_or_default();
    let priorities = locale_font_priorities(&locale);

    let fonts: Vec<String> = priorities
        .into_iter()
        .flat_map(os_font_candidates)
        .collect();

    dedup_fonts(fonts)
}

fn locale_font_priorities(locale: &str) -> Vec<FontCategory> {
    let locale = locale.to_ascii_lowercase();
    let is_traditional = locale.starts_with("zh")
        && (locale.contains("tw")
            || locale.contains("hk")
            || locale.contains("mo")
            || locale.contains("hant"));
    let is_simplified = locale.starts_with("zh")
        && !is_traditional
        && (locale.contains("cn")
            || locale.contains("sg")
            || locale.contains("my")
            || locale.contains("hans")
            || locale == "zh");

    if is_traditional {
        vec![
            FontCategory::TraditionalChinese,
            FontCategory::SimplifiedChinese,
            FontCategory::English,
        ]
    } else if is_simplified {
        vec![
            FontCategory::SimplifiedChinese,
            FontCategory::TraditionalChinese,
            FontCategory::English,
        ]
    } else {
        vec![
            FontCategory::English,
            FontCategory::SimplifiedChinese,
            FontCategory::TraditionalChinese,
        ]
    }
}

fn os_font_candidates(category: FontCategory) -> Vec<String> {
    let mut fonts: Vec<&str> = Vec::new();
    match category {
        FontCategory::SimplifiedChinese => {
            #[cfg(target_os = "windows")]
            {
                fonts.extend(["Microsoft YaHei"]);
            }
            #[cfg(target_os = "macos")]
            {
                fonts.extend(["PingFang SC"]);
            }
            #[cfg(target_os = "linux")]
            {
                fonts.extend(["Noto Sans CJK SC", "Source Han Sans SC"]);
            }
        }
        FontCategory::TraditionalChinese => {
            #[cfg(target_os = "windows")]
            {
                fonts.extend(["Microsoft JhengHei"]);
            }
            #[cfg(target_os = "macos")]
            {
                fonts.extend(["PingFang TC"]);
            }
            #[cfg(target_os = "linux")]
            {
                fonts.extend(["Noto Sans CJK TC", "Source Han Sans TC"]);
            }
        }
        FontCategory::English => {
            #[cfg(target_os = "windows")]
            {
                fonts.extend(["Segoe UI", "Arial"]);
            }
            #[cfg(target_os = "macos")]
            {
                fonts.extend(["SF Pro", "Helvetica"]);
            }
            #[cfg(target_os = "linux")]
            {
                fonts.extend(["Noto Sans", "DejaVu Sans"]);
            }
        }
    }

    fonts.into_iter().map(str::to_string).collect()
}

fn dedup_fonts(fonts: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for font in fonts {
        if seen.insert(font.to_ascii_lowercase()) {
            unique.push(font);
        }
    }
    unique
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

        let documents = Self::from_bytes(path, bytes)?;
        documents
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No document found in file"))
    }

    pub fn from_bytes(path: impl Into<PathBuf>, bytes: Vec<u8>) -> anyhow::Result<Vec<Self>> {
        let path = path.into();
        Ok(vec![Self::image(path, bytes)?])
    }

    fn image(path: PathBuf, bytes: Vec<u8>) -> anyhow::Result<Self> {
        let img = ::image::load_from_memory(&bytes)?;
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
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub documents: Vec<Document>,
}

pub type AppState = Arc<RwLock<State>>;
