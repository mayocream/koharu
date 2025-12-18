use std::{io::Cursor, path::PathBuf, sync::Arc};

use anyhow::{anyhow, bail};
use image::{DynamicImage, GenericImageView, ImageFormat, RgbaImage, imageops};
use koharu_ml::font_detector::FontPrediction;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::image::SerializableDynamicImage;

const KHR_MAGIC: &[u8; 4] = b"khr!";
const KHR_FOOTER_LEN: usize = KHR_MAGIC.len() + std::mem::size_of::<u64>();
const THUMBNAIL_HEIGHT: u32 = 300;
const THUMBNAIL_WIDTH: u32 = THUMBNAIL_HEIGHT * 4 / 3; // 4:3 aspect for contact sheet
const ICON_BYTES: &[u8] = include_bytes!("../icons/Square142x142Logo.png");

static ICON_IMAGE: Lazy<RgbaImage> = Lazy::new(|| {
    image::load_from_memory(ICON_BYTES)
        .expect("failed to decode embedded icon")
        .to_rgba8()
});

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    pub font_families: Vec<String>,
    pub font_size: f32,
    pub color: [u8; 4],
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
            font_size: 0.0,
            color: [0, 0, 0, 255],
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

pub fn serialize_khr(documents: &[Document]) -> anyhow::Result<Vec<u8>> {
    let thumbnail = thumbnail_contact_sheet(documents);
    let mut thumbnail_bytes = Vec::new();
    thumbnail.write_to(&mut Cursor::new(&mut thumbnail_bytes), ImageFormat::Jpeg)?;

    let khr_bytes = postcard::to_allocvec(documents)?;
    let khr_offset = thumbnail_bytes.len() as u64;

    let mut output = thumbnail_bytes;
    output.extend_from_slice(&khr_bytes);
    output.extend_from_slice(&khr_offset.to_le_bytes());
    output.extend_from_slice(KHR_MAGIC);

    Ok(output)
}

pub fn deserialize_khr(bytes: &[u8]) -> anyhow::Result<Vec<Document>> {
    if bytes.len() >= KHR_FOOTER_LEN && &bytes[bytes.len() - KHR_MAGIC.len()..] == KHR_MAGIC {
        let offset_start = bytes.len() - KHR_FOOTER_LEN;
        let offset_bytes: [u8; 8] = bytes[offset_start..offset_start + 8]
            .try_into()
            .expect("slice with exact length");
        let khr_offset = u64::from_le_bytes(offset_bytes) as usize;
        let khr_end = bytes.len() - KHR_FOOTER_LEN;

        if khr_offset > khr_end {
            bail!("Invalid KHR offset in file");
        }

        let khr_bytes = &bytes[khr_offset..khr_end];
        return decode_postcard(khr_bytes);
    }

    // fallback to legacy format without footer/signature
    decode_postcard(bytes)
}

fn decode_postcard(bytes: &[u8]) -> anyhow::Result<Vec<Document>> {
    if let Ok(documents) = postcard::from_bytes(bytes) {
        return Ok(documents);
    }

    let document: Document = postcard::from_bytes(bytes)?;
    Ok(vec![document])
}

fn thumbnail_contact_sheet(documents: &[Document]) -> DynamicImage {
    if documents.is_empty() {
        return DynamicImage::new_rgba8(1, 1);
    }

    let mut canvas = RgbaImage::from_pixel(
        THUMBNAIL_WIDTH,
        THUMBNAIL_HEIGHT,
        image::Rgba([255, 255, 255, 255]),
    );

    // If there's only one document, fill the entire canvas with it.
    if documents.len() == 1 {
        let thumb = documents[0]
            .image
            .thumbnail(THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);
        let (thumb_w, thumb_h) = thumb.dimensions();
        let x = ((THUMBNAIL_WIDTH - thumb_w) / 2) as i64;
        let y = ((THUMBNAIL_HEIGHT - thumb_h) / 2) as i64;
        imageops::overlay(&mut canvas, &thumb.to_rgba8(), x, y);
    } else {
        // First image takes the left 1/3 of the canvas.
        let left_width = THUMBNAIL_WIDTH / 3;
        let first_thumb = documents[0].image.thumbnail(left_width, THUMBNAIL_HEIGHT);
        let (first_w, first_h) = first_thumb.dimensions();
        let first_x = ((left_width - first_w) / 2) as i64;
        let first_y = ((THUMBNAIL_HEIGHT - first_h) / 2) as i64;
        imageops::overlay(&mut canvas, &first_thumb.to_rgba8(), first_x, first_y);

        // Remaining images are packed into the right 2/3 area.
        let remaining = &documents[1..];
        if !remaining.is_empty() {
            let area_width = THUMBNAIL_WIDTH - left_width;
            let area_height = THUMBNAIL_HEIGHT;

            let cols = ((remaining.len() as f64).sqrt().ceil() as u32).max(1);
            let rows = (remaining.len() as u32).div_ceil(cols);

            let cell_w = (area_width / cols).max(1);
            let cell_h = (area_height / rows).max(1);

            for (idx, document) in remaining.iter().enumerate() {
                let thumb = document.image.thumbnail(cell_w, cell_h);
                let (thumb_w, thumb_h) = thumb.dimensions();

                let col = (idx as u32) % cols;
                let row = (idx as u32) / cols;

                let base_x = left_width as i64 + (col * cell_w) as i64;
                let base_y = (row * cell_h) as i64;
                let x = base_x + ((cell_w as i64 - thumb_w as i64) / 2);
                let y = base_y + ((cell_h as i64 - thumb_h as i64) / 2);

                imageops::overlay(&mut canvas, &thumb.to_rgba8(), x, y);
            }
        }
    }

    // overlay small app icon on the bottom-left of the composite thumbnail
    let icon = &*ICON_IMAGE;
    let icon_w = icon.width() as i64;
    let icon_h = icon.height() as i64;
    let canvas_w = canvas.width() as i64;
    let canvas_h = canvas.height() as i64;

    if canvas_w >= icon_w && canvas_h >= icon_h {
        let padding = 8i64;
        let x = padding;
        let y = (canvas_h - icon_h).max(0);
        imageops::overlay(&mut canvas, icon, x, y);
    }

    DynamicImage::ImageRgba8(canvas)
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
