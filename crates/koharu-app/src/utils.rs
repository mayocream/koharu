//! Small image-encoding helpers.

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, RgbaImage};
use koharu_core::Region;

pub trait RegionExt {
    fn clamp(&self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)>;
}

impl RegionExt for Region {
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

pub fn encode_image(image: &DynamicImage, ext: &str) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let format = ImageFormat::from_extension(ext).unwrap_or(ImageFormat::Jpeg);
    image.write_to(&mut cursor, format)?;
    Ok(buf)
}

pub fn mime_from_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

pub fn blank_rgba(width: u32, height: u32, color: image::Rgba<u8>) -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, color))
}
