use std::path::PathBuf;

use image::{DynamicImage, GrayImage, Rgba, RgbaImage};
use koharu_core::{
    Document, FontPrediction, NamedFontPrediction, SerializableDynamicImage, TextAlign, TextBlock,
    TextDirection, TextShaderEffect, TextStrokeStyle, TextStyle,
};
use koharu_psd::{PsdExportError, PsdExportOptions, TextLayerMode, export_document};

fn rgba_image(width: u32, height: u32, color: [u8; 4]) -> SerializableDynamicImage {
    SerializableDynamicImage(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        width,
        height,
        Rgba(color),
    )))
}

fn gray_image(width: u32, height: u32, value: u8) -> SerializableDynamicImage {
    SerializableDynamicImage(DynamicImage::ImageLuma8(GrayImage::from_pixel(
        width,
        height,
        image::Luma([value]),
    )))
}

fn sample_document() -> Document {
    Document {
        id: "doc-1".to_string(),
        path: PathBuf::from("sample.png"),
        name: "sample".to_string(),
        image: rgba_image(16, 12, [240, 240, 240, 255]),
        width: 16,
        height: 12,
        text_blocks: vec![
            TextBlock {
                id: "block-h".to_string(),
                x: 2.0,
                y: 3.0,
                width: 8.0,
                height: 4.0,
                translation: Some("HELLO".to_string()),
                style: Some(TextStyle {
                    font_families: vec!["ArialMT".to_string()],
                    font_size: Some(14.0),
                    color: [1, 2, 3, 255],
                    effect: Some(TextShaderEffect {
                        italic: false,
                        bold: true,
                    }),
                    stroke: Some(TextStrokeStyle::default()),
                    text_align: Some(TextAlign::Center),
                }),
                rendered: Some(rgba_image(8, 4, [255, 0, 0, 200])),
                ..Default::default()
            },
            TextBlock {
                id: "block-v".to_string(),
                x: 10.0,
                y: 1.0,
                width: 3.0,
                height: 8.0,
                translation: Some("縦書き".to_string()),
                source_direction: Some(TextDirection::Vertical),
                font_prediction: Some(FontPrediction {
                    named_fonts: vec![NamedFontPrediction {
                        index: 0,
                        name: "YuGothic-Regular".to_string(),
                        language: Some("ja".to_string()),
                        probability: 0.9,
                        serif: false,
                    }],
                    text_color: [20, 40, 60],
                    font_size_px: 13.0,
                    angle_deg: 12.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        segment: Some(gray_image(16, 12, 96)),
        inpainted: Some(rgba_image(16, 12, [220, 220, 220, 255])),
        rendered: Some(rgba_image(16, 12, [200, 210, 220, 255])),
        brush_layer: Some(rgba_image(16, 12, [0, 255, 0, 100])),
    }
}

fn count_occurrences(bytes: &[u8], needle: &[u8]) -> usize {
    bytes
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn layer_count(bytes: &[u8]) -> i16 {
    i16::from_be_bytes([bytes[42], bytes[43]])
}

#[test]
fn exports_layered_psd_with_warning_free_raster_text_by_default() {
    let document = sample_document();
    let bytes = export_document(&document, &PsdExportOptions::default()).expect("export");

    assert_eq!(&bytes[..4], b"8BPS");
    assert_eq!(&bytes[12..14], &[0, 4]);
    assert_eq!(&bytes[14..18], &[0, 0, 0, 12]);
    assert_eq!(&bytes[18..22], &[0, 0, 0, 16]);
    assert_eq!(&bytes[22..24], &[0, 8]);
    assert_eq!(&bytes[24..26], &[0, 3]);
    assert_eq!(layer_count(&bytes), -6);
    assert_eq!(count_occurrences(&bytes, b"luni"), 0);
    assert_eq!(count_occurrences(&bytes, b"TySh"), 0);
    assert!(
        bytes
            .windows("TL 001 block-h".len())
            .any(|window| window == b"TL 001 block-h")
    );
    assert!(
        bytes
            .windows("TL 002 block-v".len())
            .any(|window| window == b"TL 002 block-v")
    );
}

#[test]
fn editable_text_layers_are_opt_in() {
    let document = sample_document();
    let options = PsdExportOptions {
        text_layer_mode: TextLayerMode::Editable,
        ..Default::default()
    };

    let bytes = export_document(&document, &options).expect("export");
    assert_eq!(count_occurrences(&bytes, b"luni"), 2);
    assert_eq!(count_occurrences(&bytes, b"TySh"), 2);
}

#[test]
fn parse_smoke_test_uses_reader_crate_for_basic_validation() {
    let document = sample_document();
    let bytes = export_document(&document, &PsdExportOptions::default()).expect("export");

    let parsed = psd::Psd::from_bytes(&bytes).expect("parse");
    assert_eq!(parsed.width(), 16);
    assert_eq!(parsed.height(), 12);
}

#[test]
fn empty_translations_are_skipped() {
    let mut document = sample_document();
    document.text_blocks.push(TextBlock {
        id: "block-empty".to_string(),
        x: 0.0,
        y: 0.0,
        width: 4.0,
        height: 4.0,
        translation: Some("   ".to_string()),
        ..Default::default()
    });

    let bytes = export_document(&document, &PsdExportOptions::default()).expect("export");
    assert_eq!(layer_count(&bytes), -6);
    assert_eq!(count_occurrences(&bytes, b"TySh"), 0);
    assert!(
        !bytes
            .windows("TL 003 block-empty".len())
            .any(|window| window == b"TL 003 block-empty")
    );
}

#[test]
fn dimensions_above_classic_psd_limit_fail() {
    let mut document = sample_document();
    document.width = 30_001;

    let error = export_document(&document, &PsdExportOptions::default()).expect_err("too large");
    assert!(matches!(
        error,
        PsdExportError::UnsupportedDimensions {
            width: 30_001,
            height: 12
        }
    ));
}

#[test]
fn missing_rendered_text_bitmap_still_exports_editable_layer() {
    let mut document = sample_document();
    document.text_blocks[0].rendered = None;
    let options = PsdExportOptions {
        text_layer_mode: TextLayerMode::Editable,
        ..Default::default()
    };

    let bytes = export_document(&document, &options).expect("export");
    assert_eq!(count_occurrences(&bytes, b"TySh"), 2);
}

#[test]
fn optional_helper_layers_can_be_disabled() {
    let document = sample_document();
    let options = PsdExportOptions {
        include_original: false,
        include_inpainted: false,
        include_segment_mask: false,
        include_brush_layer: false,
        text_layer_mode: TextLayerMode::Rasterized,
    };

    let bytes = export_document(&document, &options).expect("export");
    assert_eq!(layer_count(&bytes), -2);
    assert_eq!(count_occurrences(&bytes, b"luni"), 0);
    assert_eq!(count_occurrences(&bytes, b"TySh"), 0);
}
