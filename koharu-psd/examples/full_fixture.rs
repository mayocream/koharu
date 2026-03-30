use std::{error::Error, fs, path::PathBuf};

use image::{DynamicImage, GrayImage, Luma, Rgba, RgbaImage};
use koharu_app::renderer::Renderer;
use koharu_core::{
    Document, FontPrediction, NamedFontPrediction, SerializableDynamicImage, TextAlign, TextBlock,
    TextDirection, TextShaderEffect, TextStyle,
};
use koharu_psd::{PsdExportOptions, TextLayerMode, export_document};

fn main() -> Result<(), Box<dyn Error>> {
    let (output, editable) = parse_args()?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut document = build_document();
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

    println!(
        "mode   {}",
        match options.text_layer_mode {
            TextLayerMode::Rasterized => "rasterized",
            TextLayerMode::Editable => "editable",
        }
    );
    println!("wrote {}", output.display());
    Ok(())
}

fn parse_args() -> Result<(PathBuf, bool), Box<dyn Error>> {
    let mut output = None;
    let mut editable = false;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--editable" => editable = true,
            other if output.is_none() => output = Some(PathBuf::from(other)),
            _ => {
                return Err(
                    "usage: cargo run -p koharu-psd --example full_fixture -- [--editable] [output.psd]"
                        .into(),
                );
            }
        }
    }

    Ok((
        output.unwrap_or_else(|| PathBuf::from("target/koharu-psd/full-fixture.psd")),
        editable,
    ))
}

fn build_document() -> Document {
    let width = 960;
    let height = 640;

    let original = build_original(width, height);
    let inpainted = build_inpainted(&original);
    let segment = build_segment(width, height);
    let brush_layer = build_brush_layer(width, height);

    Document {
        id: "full-fixture".to_string(),
        path: PathBuf::from("full-fixture.png"),
        name: "full-fixture".to_string(),
        image: to_serializable_rgba(original),
        width,
        height,
        revision: 0,
        text_blocks: vec![
            TextBlock {
                id: "hero-title".to_string(),
                x: 130.0,
                y: 84.0,
                width: 300.0,
                height: 110.0,
                translation: Some("Editable PSD text".to_string()),
                style: Some(TextStyle {
                    font_families: vec!["ArialMT".to_string()],
                    font_size: Some(44.0),
                    color: [25, 24, 28, 255],
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
                x: 760.0,
                y: 170.0,
                width: 110.0,
                height: 250.0,
                translation: Some("\u{7e26}\u{66f8}\u{304d}".to_string()),
                source_direction: Some(TextDirection::Vertical),
                style: Some(TextStyle {
                    font_families: vec!["YuGothic-Regular".to_string()],
                    font_size: Some(36.0),
                    color: [28, 50, 92, 255],
                    effect: None,
                    stroke: None,
                    text_align: None,
                }),
                font_prediction: Some(FontPrediction {
                    named_fonts: vec![NamedFontPrediction {
                        index: 0,
                        name: "YuGothic-Regular".to_string(),
                        language: Some("ja".to_string()),
                        probability: 0.94,
                        serif: false,
                    }],
                    direction: TextDirection::Vertical,
                    text_color: [28, 50, 92],
                    font_size_px: 36.0,
                    angle_deg: 4.0,
                    ..Default::default()
                }),
                lock_layout_box: true,
                ..Default::default()
            },
        ],
        segment: Some(to_serializable_luma(segment)),
        inpainted: Some(to_serializable_rgba(inpainted)),
        rendered: None,
        brush_layer: Some(to_serializable_rgba(brush_layer)),
    }
}

fn build_original(width: u32, height: u32) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let warm = 210u8.saturating_add((x / 32 % 2) as u8 * 12);
            let cool = 190u8.saturating_add((y / 24 % 2) as u8 * 18);
            let blue = 170u8.saturating_add(((x + y) / 48 % 2) as u8 * 20);
            image.put_pixel(x, y, Rgba([warm, cool, blue, 255]));
        }
    }
    image
}

fn build_inpainted(original: &RgbaImage) -> RgbaImage {
    let mut image = original.clone();
    for y in 210..455 {
        for x in 250..700 {
            image.put_pixel(x, y, Rgba([244, 240, 230, 255]));
        }
    }
    image
}

fn build_segment(width: u32, height: u32) -> GrayImage {
    let mut image = GrayImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let inside = (120..440).contains(&x) && (60..220).contains(&y)
                || (730..890).contains(&x) && (150..470).contains(&y)
                || (240..720).contains(&x) && (210..470).contains(&y);
            image.put_pixel(x, y, Luma([if inside { 180 } else { 32 }]));
        }
    }
    image
}

fn build_brush_layer(width: u32, height: u32) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    for x in 240..720 {
        for thickness in 0..8 {
            image.put_pixel(x, 208 + thickness, Rgba([255, 0, 128, 150]));
            image.put_pixel(x, 468 - thickness, Rgba([255, 0, 128, 150]));
        }
    }
    for y in 208..468 {
        for thickness in 0..8 {
            image.put_pixel(240 + thickness, y, Rgba([255, 0, 128, 150]));
            image.put_pixel(712 - thickness, y, Rgba([255, 0, 128, 150]));
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
