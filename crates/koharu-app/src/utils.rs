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

pub fn format_sources(sources: &[String]) -> String {
    sources
        .iter()
        .enumerate()
        .map(|(idx, text)| format!("[{}]{}", idx + 1, text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_block_tag(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    if bytes.first()? != &b'[' {
        return None;
    }
    let end = text[1..].find(']')?;
    let num_str = &text[1..1 + end];
    let id_1based: usize = num_str.parse().ok()?;
    if id_1based == 0 {
        return None;
    }
    Some((1 + end + 1, id_1based - 1))
}

fn find_next_tag(text: &str) -> Option<(usize, usize, usize)> {
    let mut line_start = 0;
    while line_start <= text.len() {
        let line = &text[line_start..];
        let indent = line
            .as_bytes()
            .iter()
            .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
            .count();
        let offset = line_start + indent;
        if let Some((len, id)) = parse_block_tag(&text[offset..]) {
            return Some((offset, len, id));
        }
        let Some(next_newline) = line.find('\n') else {
            break;
        };
        line_start += next_newline + 1;
    }
    None
}

pub fn parse_tagged_blocks(
    translation: &str,
    expected_blocks: usize,
) -> anyhow::Result<Option<Vec<String>>> {
    if find_next_tag(translation).is_none() {
        return Ok(None);
    }
    let mut blocks = vec![String::new(); expected_blocks];
    let mut cursor = translation;
    let mut found_any = false;
    while let Some((offset, len, id)) = find_next_tag(cursor) {
        found_any = true;
        cursor = &cursor[offset + len..];
        let content_end = find_next_tag(cursor)
            .map(|(next_offset, _, _)| next_offset)
            .unwrap_or(cursor.len());
        let content = cursor[..content_end].trim().to_string();
        if id < expected_blocks {
            blocks[id] = content;
        }
        cursor = &cursor[content_end..];
    }
    Ok(found_any.then_some(blocks))
}
