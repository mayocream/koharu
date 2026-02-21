use std::{io::Cursor, path::PathBuf};

use image::{ImageFormat, RgbaImage};
use koharu_api::commands::InpaintRegion;
use koharu_types::{Document, SerializableDynamicImage};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

pub trait InpaintRegionExt {
    fn clamp(&self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)>;
}

impl InpaintRegionExt for InpaintRegion {
    fn clamp(&self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
        if width == 0 || height == 0 {
            return None;
        }
        let x0 = self.x.min(width.saturating_sub(1));
        let y0 = self.y.min(height.saturating_sub(1));
        let x1 = self.x.saturating_add(self.width).min(width).max(x0);
        let y1 = self.y.saturating_add(self.height).min(height).max(y0);
        let w = x1.saturating_sub(x0);
        let h = y1.saturating_sub(y0);
        if w == 0 || h == 0 {
            return None;
        }
        Some((x0, y0, w, h))
    }
}

pub(crate) fn encode_image(image: &SerializableDynamicImage, ext: &str) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let format = ImageFormat::from_extension(ext).unwrap_or(ImageFormat::Jpeg);
    image.0.write_to(&mut cursor, format)?;
    Ok(buf)
}

pub(crate) fn mime_from_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

pub(crate) fn blank_rgba(
    width: u32,
    height: u32,
    color: image::Rgba<u8>,
) -> SerializableDynamicImage {
    let blank = RgbaImage::from_pixel(width, height, color);
    image::DynamicImage::ImageRgba8(blank).into()
}

pub fn load_documents(inputs: Vec<(PathBuf, Vec<u8>)>) -> anyhow::Result<Vec<Document>> {
    if inputs.is_empty() {
        return Ok(vec![]);
    }

    let mut documents: Vec<_> = inputs
        .into_par_iter()
        .filter_map(|(path, bytes)| match Document::from_bytes(path, bytes) {
            Ok(docs) => Some(docs),
            Err(err) => {
                tracing::warn!(?err, "Failed to parse document");
                None
            }
        })
        .flatten()
        .collect();

    documents.sort_by_key(|doc| doc.name.clone());
    Ok(documents)
}
