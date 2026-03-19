use std::{error::Error, fs, path::PathBuf};

use image::{DynamicImage, GrayImage, Luma, Rgba, RgbaImage};
use koharu_psd::{PsdExportOptions, TextLayerMode, export_document};
use koharu_renderer::facade::Renderer;
use koharu_types::{
    Document, FontPrediction, NamedFontPrediction, SerializableDynamicImage, TextAlign, TextBlock,
    TextDirection, TextShaderEffect, TextStyle,
};

fn main() -> Result<(), Box<dyn Error>> {
    let (input, output, editable) = parse_args()?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut document = Document::open(input.clone())?;
    apply_demo_layers(&mut document);
    render_demo_text(&mut document)?;

    let options = PsdExportOptions {
        text_layer_mode: if editable {
            TextLayerMode::Editable
        } else {
            TextLayerMode::Rasterized
        },
        ..Default::default()
    };

    let bytes = export_document(&document, &options)?;
    fs::write(&output, bytes)?;

    println!("input  {}", input.display());
    println!(
        "mode   {}",
        match options.text_layer_mode {
            TextLayerMode::Rasterized => "rasterized",
            TextLayerMode::Editable => "editable",
        }
    );
    println!("wrote  {}", output.display());
    Ok(())
}

fn parse_args() -> Result<(PathBuf, PathBuf, bool), Box<dyn Error>> {
    let mut input = None;
    let mut output = None;
    let mut editable = false;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--editable" => editable = true,
            other if input.is_none() => input = Some(PathBuf::from(other)),
            other if output.is_none() => output = Some(PathBuf::from(other)),
            _ => {
                return Err(
                    "usage: cargo run -p koharu-psd --example from_image -- [--editable] <input-image> [output.psd]"
                        .into(),
                );
            }
        }
    }

    let input = input.ok_or_else(|| {
        "usage: cargo run -p koharu-psd --example from_image -- [--editable] <input-image> [output.psd]"
            .to_string()
    })?;
    let output = output.unwrap_or_else(|| input.with_extension("psd"));
    Ok((input, output, editable))
}

fn apply_demo_layers(document: &mut Document) {
    let width = document.width.max(document.image.width());
    let height = document.height.max(document.image.height());
    document.width = width;
    document.height = height;

    let base = document.image.to_rgba8();
    let inpainted = softened_copy(&base);
    let segment = build_segment(width, height);
    let brush = build_brush(width, height);

    document.inpainted = Some(to_serializable_rgba(inpainted));
    document.segment = Some(to_serializable_luma(segment));
    document.brush_layer = Some(to_serializable_rgba(brush));
    document.rendered = None;
    document.text_blocks = vec![
        TextBlock {
            id: "top-callout".to_string(),
            x: (width as f32 * 0.08).floor(),
            y: (height as f32 * 0.08).floor(),
            width: width.min(420).max(180) as f32,
            height: (height / 5).max(72) as f32,
            translation: Some("Generated from your image".to_string()),
            style: Some(TextStyle {
                font_families: vec!["ArialMT".to_string()],
                font_size: Some((height as f32 * 0.06).max(18.0)),
                color: [24, 24, 28, 255],
                effect: Some(TextShaderEffect {
                    italic: false,
                    bold: true,
                }),
                stroke: None,
                text_align: Some(TextAlign::Center),
            }),
            lock_layout_box: true,
            ..Default::default()
        },
        TextBlock {
            id: "side-note".to_string(),
            x: (width as f32 * 0.78).floor(),
            y: (height as f32 * 0.18).floor(),
            width: (width as f32 * 0.14).max(64.0),
            height: (height as f32 * 0.42).max(180.0),
            translation: Some("\u{7e26}\u{66f8}\u{304d}".to_string()),
            source_direction: Some(TextDirection::Vertical),
            style: Some(TextStyle {
                font_families: vec!["YuGothic-Regular".to_string()],
                font_size: Some((height as f32 * 0.05).max(18.0)),
                color: [26, 54, 96, 255],
                effect: None,
                stroke: None,
                text_align: None,
            }),
            font_prediction: Some(FontPrediction {
                named_fonts: vec![NamedFontPrediction {
                    index: 0,
                    name: "YuGothic-Regular".to_string(),
                    language: Some("ja".to_string()),
                    probability: 0.9,
                    serif: false,
                }],
                direction: TextDirection::Vertical,
                text_color: [26, 54, 96],
                font_size_px: (height as f32 * 0.05).max(18.0),
                angle_deg: 6.0,
                ..Default::default()
            }),
            lock_layout_box: true,
            ..Default::default()
        },
    ];
}

fn softened_copy(base: &RgbaImage) -> RgbaImage {
    let mut out = base.clone();
    for pixel in out.pixels_mut() {
        pixel.0[0] = ((u16::from(pixel.0[0]) * 9 + 255) / 10) as u8;
        pixel.0[1] = ((u16::from(pixel.0[1]) * 9 + 248) / 10) as u8;
        pixel.0[2] = ((u16::from(pixel.0[2]) * 9 + 240) / 10) as u8;
    }
    out
}

fn build_segment(width: u32, height: u32) -> GrayImage {
    let mut image = GrayImage::new(width, height);
    let x0 = width / 10;
    let x1 = width.saturating_mul(9) / 10;
    let y0 = height / 12;
    let y1 = height.saturating_mul(11) / 12;

    for y in 0..height {
        for x in 0..width {
            let bright = (x0..x1).contains(&x) && (y0..y1).contains(&y);
            image.put_pixel(x, y, Luma([if bright { 150 } else { 24 }]));
        }
    }
    image
}

fn build_brush(width: u32, height: u32) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    let left = width / 8;
    let right = width.saturating_mul(7) / 8;
    let top = height.saturating_mul(3) / 5;

    for x in left..right {
        for offset in 0..6 {
            let y = top + offset;
            if y < height {
                image.put_pixel(x, y, Rgba([255, 0, 128, 120]));
            }
        }
    }

    image
}

fn render_demo_text(document: &mut Document) -> Result<(), Box<dyn Error>> {
    Renderer::new()?.render(document, None, TextShaderEffect::none(), None, None)?;
    Ok(())
}

fn to_serializable_rgba(image: RgbaImage) -> SerializableDynamicImage {
    SerializableDynamicImage(DynamicImage::ImageRgba8(image))
}

fn to_serializable_luma(image: GrayImage) -> SerializableDynamicImage {
    SerializableDynamicImage(DynamicImage::ImageLuma8(image))
}
