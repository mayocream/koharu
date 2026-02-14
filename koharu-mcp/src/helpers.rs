use std::io::Cursor;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use image::{DynamicImage, ImageFormat};

pub(crate) fn encode_png_base64(img: &DynamicImage, max_size: u32) -> String {
    let img = if img.width().max(img.height()) > max_size {
        img.resize(max_size, max_size, image::imageops::FilterType::Lanczos3)
    } else {
        img.clone()
    };
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .expect("PNG encoding failed");
    BASE64.encode(&buf)
}

pub(crate) fn parse_hex_color(hex: &str) -> Result<[u8; 4], String> {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).map_err(|e| e.to_string())?;
            let g = u8::from_str_radix(&hex[2..4], 16).map_err(|e| e.to_string())?;
            let b = u8::from_str_radix(&hex[4..6], 16).map_err(|e| e.to_string())?;
            Ok([r, g, b, 255])
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).map_err(|e| e.to_string())?;
            let g = u8::from_str_radix(&hex[2..4], 16).map_err(|e| e.to_string())?;
            let b = u8::from_str_radix(&hex[4..6], 16).map_err(|e| e.to_string())?;
            let a = u8::from_str_radix(&hex[6..8], 16).map_err(|e| e.to_string())?;
            Ok([r, g, b, a])
        }
        _ => Err(format!("Invalid hex color: {hex}")),
    }
}

pub(crate) fn parse_shader_effect(s: &str) -> Result<koharu_types::TextShaderEffect, String> {
    match s.to_lowercase().as_str() {
        "normal" => Ok(koharu_types::TextShaderEffect::Normal),
        "antique" => Ok(koharu_types::TextShaderEffect::Antique),
        "metal" => Ok(koharu_types::TextShaderEffect::Metal),
        "manga" => Ok(koharu_types::TextShaderEffect::Manga),
        "motionblur" | "motion_blur" => Ok(koharu_types::TextShaderEffect::MotionBlur),
        _ => Err(format!(
            "Unknown shader effect: {s}. Valid: normal, antique, metal, manga, motionblur"
        )),
    }
}
