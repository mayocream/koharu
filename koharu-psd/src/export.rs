use std::io::Write;

use image::{DynamicImage, GrayImage, Rgba, RgbaImage, imageops::overlay};

use crate::{
    descriptor::{
        DescriptorObject, DescriptorValue, bounds_descriptor, write_versioned_descriptor,
    },
    engine_data::{TextEngineSpec, TextJustification, TextOrientation, encode_engine_data},
    error::PsdExportError,
    input::{
        PsdBlobRef, PsdDocument, PsdFontPrediction, PsdTextAlign, PsdTextBlock, PsdTextDirection,
        ResolvedDocument,
    },
    packbits::{ChannelId, encode_image_rle},
    writer::PsdWriter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextLayerMode {
    Rasterized,
    Editable,
}

#[derive(Debug, Clone)]
pub struct PsdExportOptions {
    pub include_original: bool,
    pub include_inpainted: bool,
    pub include_segment_mask: bool,
    pub include_brush_layer: bool,
    pub text_layer_mode: TextLayerMode,
}

impl Default for PsdExportOptions {
    fn default() -> Self {
        Self {
            include_original: true,
            include_inpainted: true,
            include_segment_mask: true,
            include_brush_layer: true,
            text_layer_mode: TextLayerMode::Rasterized,
        }
    }
}

#[derive(Debug, Clone)]
struct ExportLayer {
    name: String,
    left: i32,
    top: i32,
    pixels: RgbaImage,
    hidden: bool,
    text: Option<TextLayerMetadata>,
}

#[derive(Debug, Clone)]
struct TextLayerMetadata {
    index: i32,
    text: String,
    bounds: [f64; 4],
    transform: [f64; 6],
    orientation: TextOrientation,
    justification: TextJustification,
    font_name: String,
    font_size: f64,
    color: [u8; 4],
    faux_bold: bool,
    faux_italic: bool,
    box_width: f64,
    box_height: f64,
}

pub fn export_document(
    resolved: &ResolvedDocument,
    options: &PsdExportOptions,
) -> Result<Vec<u8>, PsdExportError> {
    let mut bytes = Vec::new();
    write_document(&mut bytes, resolved, options)?;
    Ok(bytes)
}

pub fn write_document<W: Write>(
    mut writer: W,
    resolved: &ResolvedDocument,
    options: &PsdExportOptions,
) -> Result<(), PsdExportError> {
    let document = resolved.document;
    let (width, height) = document_dimensions(document)?;
    let layers_bottom_to_top = collect_layers(resolved, options)?;
    let composite = merged_composite(resolved, &layers_bottom_to_top, width, height);
    let layers_top_to_bottom: Vec<&ExportLayer> = layers_bottom_to_top.iter().rev().collect();

    let mut psd = PsdWriter::new();
    write_header(&mut psd, width, height);
    psd.write_u32(0);
    psd.write_u32(0);

    let layer_mask_info = build_layer_and_mask_info(&layers_top_to_bottom)?;
    psd.write_u32(layer_mask_info.len() as u32);
    psd.write_bytes(&layer_mask_info);

    write_image_data(&mut psd, &composite, "Merged Composite")?;

    writer.write_all(&psd.into_inner())?;
    Ok(())
}

fn document_dimensions(document: &PsdDocument) -> Result<(u32, u32), PsdExportError> {
    let width = document.width;
    let height = document.height;

    if width == 0 || height == 0 {
        return Err(PsdExportError::MissingBaseImage);
    }

    if width > 30_000 || height > 30_000 {
        return Err(PsdExportError::UnsupportedDimensions { width, height });
    }

    Ok((width, height))
}

fn write_header(writer: &mut PsdWriter, width: u32, height: u32) {
    writer.write_signature("8BPS");
    writer.write_u16(1);
    writer.write_zeroes(6);
    writer.write_u16(4);
    writer.write_u32(height);
    writer.write_u32(width);
    writer.write_u16(8);
    writer.write_u16(3);
}

fn collect_layers(
    resolved: &ResolvedDocument,
    options: &PsdExportOptions,
) -> Result<Vec<ExportLayer>, PsdExportError> {
    let document = resolved.document;
    let mut layers = Vec::new();
    let include_inpainted = options.include_inpainted && resolved.inpainted.is_some();

    if options.include_original {
        let pixels = dynamic_to_rgba(resolved.source);
        validate_layer_pixels("Original Image", &pixels)?;
        layers.push(ExportLayer {
            name: "Original Image".to_string(),
            left: 0,
            top: 0,
            pixels,
            hidden: include_inpainted,
            text: None,
        });
    }

    if let Some(image) = resolved.inpainted.filter(|_| options.include_inpainted) {
        let pixels = dynamic_to_rgba(image);
        validate_layer_pixels("Inpainted", &pixels)?;
        layers.push(ExportLayer {
            name: "Inpainted".to_string(),
            left: 0,
            top: 0,
            pixels,
            hidden: false,
            text: None,
        });
    }

    if let Some(mask) = resolved.segment.filter(|_| options.include_segment_mask) {
        let pixels = grayscale_mask_rgba(mask);
        validate_layer_pixels("Segmentation Mask", &pixels)?;
        layers.push(ExportLayer {
            name: "Segmentation Mask".to_string(),
            left: 0,
            top: 0,
            pixels,
            hidden: true,
            text: None,
        });
    }

    if let Some(brush) = resolved.brush_layer.filter(|_| options.include_brush_layer) {
        let pixels = dynamic_to_rgba(brush);
        validate_layer_pixels("Brush Layer", &pixels)?;
        layers.push(ExportLayer {
            name: "Brush Layer".to_string(),
            left: 0,
            top: 0,
            pixels,
            hidden: false,
            text: None,
        });
    }

    let mut text_index = 1i32;
    for block in &document.text_blocks {
        if let Some(layer) = text_layer(
            block,
            text_index,
            options.text_layer_mode,
            resolved.block_images,
        )? {
            layers.push(layer);
            text_index += 1;
        }
    }

    Ok(layers)
}

fn text_layer(
    block: &PsdTextBlock,
    index: i32,
    mode: TextLayerMode,
    block_images: &std::collections::HashMap<PsdBlobRef, DynamicImage>,
) -> Result<Option<ExportLayer>, PsdExportError> {
    let text = block.translation.clone().unwrap_or_default();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let left = block.x.trunc() as i32;
    let top = block.y.trunc() as i32;

    let pixels =
        if let Some(rendered_img) = block.rendered.as_ref().and_then(|r| block_images.get(r)) {
            dynamic_to_rgba(rendered_img)
        } else {
            let width = block.width.ceil().max(1.0) as u32;
            let height = block.height.ceil().max(1.0) as u32;
            RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]))
        };
    validate_layer_pixels(&block.id, &pixels)?;

    let text = match mode {
        TextLayerMode::Rasterized => None,
        TextLayerMode::Editable => {
            let orientation = infer_orientation(block);
            let justification = infer_justification(block, trimmed, orientation);
            let font_name = infer_font_name(block);
            let font_size = infer_font_size(block);
            let color = infer_color(block);
            let faux_bold = block
                .style
                .as_ref()
                .and_then(|style| style.effect)
                .map(|effect| effect.bold)
                .unwrap_or(false);
            let faux_italic = block
                .style
                .as_ref()
                .and_then(|style| style.effect)
                .map(|effect| effect.italic)
                .unwrap_or(false);
            let rotation_deg = block
                .rotation_deg
                .or_else(|| {
                    block
                        .font_prediction
                        .as_ref()
                        .map(|prediction| prediction.angle_deg)
                })
                .unwrap_or(0.0) as f64;
            let rotation_rad = rotation_deg.to_radians();
            let transform = [
                rotation_rad.cos(),
                rotation_rad.sin(),
                -rotation_rad.sin(),
                rotation_rad.cos(),
                block.x as f64,
                block.y as f64,
            ];
            let bounds = [
                block.x as f64,
                block.y as f64,
                block.x as f64 + block.width as f64,
                block.y as f64 + block.height as f64,
            ];

            Some(TextLayerMetadata {
                index,
                text: trimmed.to_string(),
                bounds,
                transform,
                orientation,
                justification,
                font_name,
                font_size,
                color,
                faux_bold,
                faux_italic,
                box_width: block.width.max(1.0) as f64,
                box_height: block.height.max(1.0) as f64,
            })
        }
    };

    Ok(Some(ExportLayer {
        name: format!("TL {index:03} {}", block.id),
        left,
        top,
        pixels,
        hidden: false,
        text,
    }))
}

fn validate_layer_pixels(layer: &str, pixels: &RgbaImage) -> Result<(), PsdExportError> {
    let width = pixels.width() as i32;
    let height = pixels.height() as i32;
    if width <= 0 || height <= 0 {
        return Err(PsdExportError::InvalidLayerBounds {
            layer: layer.to_string(),
            width,
            height,
        });
    }
    Ok(())
}

fn dynamic_to_rgba(image: &DynamicImage) -> RgbaImage {
    image.to_rgba8()
}

fn grayscale_mask_rgba(image: &DynamicImage) -> RgbaImage {
    let mask: GrayImage = image.to_luma8();
    let mut rgba = RgbaImage::new(mask.width(), mask.height());
    for (x, y, pixel) in mask.enumerate_pixels() {
        rgba.put_pixel(x, y, Rgba([pixel[0], pixel[0], pixel[0], 255]));
    }
    rgba
}

fn merged_composite(
    resolved: &ResolvedDocument,
    layers_bottom_to_top: &[ExportLayer],
    width: u32,
    height: u32,
) -> RgbaImage {
    if let Some(rendered) = resolved.rendered {
        return place_on_canvas(&rendered.to_rgba8(), width, height);
    }

    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    for layer in layers_bottom_to_top.iter().filter(|layer| !layer.hidden) {
        overlay(
            &mut canvas,
            &layer.pixels,
            i64::from(layer.left),
            i64::from(layer.top),
        );
    }
    canvas
}

fn place_on_canvas(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if image.width() == width && image.height() == height {
        return image.clone();
    }

    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    overlay(&mut canvas, image, 0, 0);
    canvas
}

fn build_layer_and_mask_info(layers: &[&ExportLayer]) -> Result<Vec<u8>, PsdExportError> {
    let mut layer_info = PsdWriter::new();
    if layers.is_empty() {
        layer_info.write_i16(0);
    } else {
        layer_info.write_i16(-(layers.len() as i16));
    }

    let mut encoded_layers = Vec::with_capacity(layers.len());
    let mut extra_data = Vec::with_capacity(layers.len());

    for layer in layers {
        let channels = encode_image_rle(
            &layer.pixels,
            &[
                ChannelId::Red,
                ChannelId::Green,
                ChannelId::Blue,
                ChannelId::Alpha,
            ],
            &layer.name,
        )?;
        let extra = build_extra_data(layer)?;
        encoded_layers.push(channels);
        extra_data.push(extra);
    }

    for ((layer, channels), extra) in layers.iter().zip(&encoded_layers).zip(&extra_data) {
        let width = i32::try_from(layer.pixels.width()).map_err(|_| {
            PsdExportError::InvalidLayerBounds {
                layer: layer.name.clone(),
                width: i32::MAX,
                height: layer.pixels.height() as i32,
            }
        })?;
        let height = i32::try_from(layer.pixels.height()).map_err(|_| {
            PsdExportError::InvalidLayerBounds {
                layer: layer.name.clone(),
                width,
                height: i32::MAX,
            }
        })?;
        let right =
            layer
                .left
                .checked_add(width)
                .ok_or_else(|| PsdExportError::InvalidLayerBounds {
                    layer: layer.name.clone(),
                    width,
                    height,
                })?;
        let bottom =
            layer
                .top
                .checked_add(height)
                .ok_or_else(|| PsdExportError::InvalidLayerBounds {
                    layer: layer.name.clone(),
                    width,
                    height,
                })?;

        layer_info.write_i32(layer.top);
        layer_info.write_i32(layer.left);
        layer_info.write_i32(bottom);
        layer_info.write_i32(right);
        layer_info.write_u16(channels.len() as u16);

        for channel in channels {
            layer_info.write_i16(channel.channel_id);
            layer_info.write_u32((2 + channel.data.len()) as u32);
        }

        layer_info.write_signature("8BIM");
        layer_info.write_signature("norm");
        layer_info.write_u8(255);
        layer_info.write_u8(0);
        layer_info.write_u8(if layer.hidden { 0x0A } else { 0x08 });
        layer_info.write_u8(0);
        layer_info.write_u32(extra.len() as u32);
        layer_info.write_bytes(extra);
    }

    for channels in &encoded_layers {
        for channel in channels {
            layer_info.write_u16(1);
            layer_info.write_bytes(&channel.data);
        }
    }
    layer_info.pad_to_multiple(4);

    let mut full = PsdWriter::new();
    full.write_u32(layer_info.len() as u32);
    full.write_bytes(&layer_info.into_inner());
    full.write_u32(0);
    Ok(full.into_inner())
}

fn build_extra_data(layer: &ExportLayer) -> Result<Vec<u8>, PsdExportError> {
    let mut extra = PsdWriter::new();
    extra.write_u32(0);
    extra.write_u32(0);
    extra.write_pascal_string(&layer.name, 4);

    if let Some(text) = layer.text.as_ref() {
        write_additional_info_block(&mut extra, "luni", &luni_body(&layer.name), 4);
        write_additional_info_block(&mut extra, "TySh", &tysh_body(text)?, 2);
    }

    Ok(extra.into_inner())
}

fn luni_body(name: &str) -> Vec<u8> {
    let mut body = PsdWriter::new();
    body.write_unicode_string(name);
    body.into_inner()
}

fn tysh_body(text: &TextLayerMetadata) -> Result<Vec<u8>, PsdExportError> {
    let engine_data = encode_engine_data(&TextEngineSpec {
        text: text.text.clone(),
        font_name: text.font_name.clone(),
        font_size: text.font_size,
        color: text.color,
        faux_bold: text.faux_bold,
        faux_italic: text.faux_italic,
        orientation: text.orientation,
        justification: text.justification,
        box_width: text.box_width,
        box_height: text.box_height,
    });

    let bounds = bounds_descriptor(
        "bounds",
        text.bounds[0],
        text.bounds[1],
        text.bounds[2],
        text.bounds[3],
    );
    let bounding_box = bounds_descriptor(
        "boundingBox",
        text.bounds[0],
        text.bounds[1],
        text.bounds[2],
        text.bounds[3],
    );

    let text_descriptor = DescriptorObject::new("", "TxLr")
        .with_item("Txt ", DescriptorValue::Text(text.text.clone()))
        .with_item(
            "textGridding",
            DescriptorValue::Enum {
                type_id: "textGridding".to_string(),
                value: "None".to_string(),
            },
        )
        .with_item(
            "Ornt",
            DescriptorValue::Enum {
                type_id: "Ornt".to_string(),
                value: match text.orientation {
                    TextOrientation::Horizontal => "Hrzn".to_string(),
                    TextOrientation::Vertical => "Vrtc".to_string(),
                },
            },
        )
        .with_item(
            "AntA",
            DescriptorValue::Enum {
                type_id: "Annt".to_string(),
                value: "antiAliasSharp".to_string(),
            },
        )
        .with_item("bounds", DescriptorValue::Object(bounds))
        .with_item("boundingBox", DescriptorValue::Object(bounding_box))
        .with_item("TextIndex", DescriptorValue::Integer(text.index))
        .with_item("EngineData", DescriptorValue::Raw(engine_data));

    let warp_descriptor = DescriptorObject::new("", "warp")
        .with_item(
            "warpStyle",
            DescriptorValue::Enum {
                type_id: "warpStyle".to_string(),
                value: "warpNone".to_string(),
            },
        )
        .with_item("warpValue", DescriptorValue::Double(0.0))
        .with_item("warpPerspective", DescriptorValue::Double(0.0))
        .with_item("warpPerspectiveOther", DescriptorValue::Double(0.0))
        .with_item(
            "warpRotate",
            DescriptorValue::Enum {
                type_id: "Ornt".to_string(),
                value: match text.orientation {
                    TextOrientation::Horizontal => "Hrzn".to_string(),
                    TextOrientation::Vertical => "Vrtc".to_string(),
                },
            },
        )
        .with_item(
            "bounds",
            DescriptorValue::Object(bounds_descriptor(
                "bounds",
                text.bounds[0],
                text.bounds[1],
                text.bounds[2],
                text.bounds[3],
            )),
        );

    let mut body = PsdWriter::new();
    body.write_i16(1);
    for value in text.transform {
        body.write_f64(value);
    }
    body.write_i16(50);
    write_versioned_descriptor(&mut body, &text_descriptor)?;
    body.write_i16(1);
    write_versioned_descriptor(&mut body, &warp_descriptor)?;
    for value in text.bounds {
        body.write_f32(value as f32);
    }
    Ok(body.into_inner())
}

fn write_additional_info_block(writer: &mut PsdWriter, key: &str, body: &[u8], alignment: usize) {
    let padding = (alignment - (body.len() % alignment)) % alignment;

    writer.write_signature("8BIM");
    writer.write_signature(key);
    writer.write_u32((body.len() + padding) as u32);
    writer.write_bytes(body);
    writer.write_zeroes(padding);
}

fn write_image_data(
    writer: &mut PsdWriter,
    image: &RgbaImage,
    name: &str,
) -> Result<(), PsdExportError> {
    writer.write_u16(1);
    let channels = encode_image_rle(
        image,
        &[
            ChannelId::Red,
            ChannelId::Green,
            ChannelId::Blue,
            ChannelId::Alpha,
        ],
        name,
    )?;

    let row_lengths_len = image.height() as usize * 2;
    for channel in &channels {
        writer.write_bytes(&channel.data[..row_lengths_len]);
    }
    for channel in &channels {
        writer.write_bytes(&channel.data[row_lengths_len..]);
    }
    Ok(())
}

fn infer_orientation(block: &PsdTextBlock) -> TextOrientation {
    match block.rendered_direction.or(block.source_direction) {
        Some(PsdTextDirection::Vertical) => TextOrientation::Vertical,
        _ => TextOrientation::Horizontal,
    }
}

fn infer_justification(
    block: &PsdTextBlock,
    text: &str,
    orientation: TextOrientation,
) -> TextJustification {
    if let Some(alignment) = block.style.as_ref().and_then(|style| style.text_align) {
        return match alignment {
            PsdTextAlign::Left => TextJustification::Left,
            PsdTextAlign::Center => TextJustification::Center,
            PsdTextAlign::Right => TextJustification::Right,
        };
    }

    if orientation == TextOrientation::Horizontal && is_probably_latin(text) {
        TextJustification::Center
    } else {
        TextJustification::Left
    }
}

fn infer_font_name(block: &PsdTextBlock) -> String {
    if let Some(style_font) = block.style.as_ref().and_then(|style| {
        style
            .font_families
            .iter()
            .find(|font| !font.trim().is_empty())
    }) {
        return style_font.trim().to_string();
    }

    if let Some(predicted_font) = block.font_prediction.as_ref().and_then(|prediction| {
        prediction
            .named_fonts
            .iter()
            .find(|font| !font.name.trim().is_empty())
    }) {
        return predicted_font.name.trim().to_string();
    }

    "ArialMT".to_string()
}

fn infer_font_size(block: &PsdTextBlock) -> f64 {
    if let Some(size) = block.style.as_ref().and_then(|style| style.font_size)
        && size.is_finite()
        && size > 0.0
    {
        return size as f64;
    }

    if let Some(prediction) = block.font_prediction.as_ref()
        && prediction.font_size_px.is_finite()
        && prediction.font_size_px > 0.0
    {
        return prediction.font_size_px as f64;
    }

    if let Some(size) = block.detected_font_size_px
        && size.is_finite()
        && size > 0.0
    {
        return size as f64;
    }

    f64::max(6.0, f64::from(block.width.min(block.height)) * 0.7)
}

fn infer_color(block: &PsdTextBlock) -> [u8; 4] {
    if let Some(style) = block.style.as_ref() {
        return style.color;
    }

    if let Some(PsdFontPrediction { text_color, .. }) = block.font_prediction.as_ref() {
        return [text_color[0], text_color[1], text_color[2], 255];
    }

    [0, 0, 0, 255]
}

fn contains_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x3040..=0x30FF
                | 0x3400..=0x4DBF
                | 0x4E00..=0x9FFF
                | 0xAC00..=0xD7AF
                | 0xF900..=0xFAFF
                | 0xFF66..=0xFF9D
        )
    })
}

fn is_probably_latin(text: &str) -> bool {
    text.chars().any(|ch| ch.is_ascii_alphabetic()) && !contains_cjk(text)
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use crate::writer::PsdWriter;

    use crate::input::{PsdTextBlock, PsdTextDirection, PsdTextStyle};

    use super::{
        TextOrientation, contains_cjk, infer_font_name, infer_orientation, is_probably_latin,
        place_on_canvas, write_image_data,
    };

    #[test]
    fn place_on_canvas_keeps_size_stable() {
        let image = RgbaImage::new(4, 4);
        let canvas = place_on_canvas(&image, 8, 6);
        assert_eq!(canvas.width(), 8);
        assert_eq!(canvas.height(), 6);
    }

    #[test]
    fn language_heuristics_detect_cjk_vs_latin() {
        assert!(contains_cjk("縦書き"));
        assert!(is_probably_latin("HELLO"));
        assert!(!is_probably_latin("縦書き"));
    }

    #[test]
    fn orientation_uses_rendered_direction_not_geometry() {
        let tall_english_block = PsdTextBlock {
            width: 40.0,
            height: 120.0,
            translation: Some("HELLO".to_string()),
            ..Default::default()
        };
        assert_eq!(
            infer_orientation(&tall_english_block),
            TextOrientation::Horizontal
        );

        let vertical_block = PsdTextBlock {
            rendered_direction: Some(PsdTextDirection::Vertical),
            ..Default::default()
        };
        assert_eq!(
            infer_orientation(&vertical_block),
            TextOrientation::Vertical
        );
    }

    #[test]
    fn composite_image_data_groups_row_tables_before_channel_payloads() {
        let mut image = RgbaImage::new(1, 1);
        image.put_pixel(0, 0, Rgba([1, 2, 3, 4]));

        let mut writer = PsdWriter::new();
        write_image_data(&mut writer, &image, "Merged Composite").expect("write image data");

        assert_eq!(
            writer.into_inner(),
            vec![
                0, 1, // compression
                0, 2, 0, 2, 0, 2, 0, 2, // row lengths
                0, 1, // red
                0, 2, // green
                0, 3, // blue
                0, 4, // alpha
            ]
        );
    }

    #[test]
    fn style_font_name_is_used_for_editable_export() {
        let block = PsdTextBlock {
            style: Some(PsdTextStyle {
                font_families: vec!["ArialMT".to_string()],
                font_size: None,
                color: [0, 0, 0, 255],
                effect: None,
                text_align: None,
            }),
            ..Default::default()
        };

        assert_eq!(infer_font_name(&block), "ArialMT");
    }
}
