//! Scene-to-PSD document projection.
//!
//! GIMP reference: layer traversal and reverse layer-record ordering:
//! https://github.com/GNOME/gimp/blob/758fb4ed995bbb339282d3f777089a33f0a391b8/plug-ins/file-psd/psd-export.c#L1520-L1880
//! GIMP reference: `TySh` transforms and descriptors consumed by the importer:
//! https://github.com/GNOME/gimp/blob/758fb4ed995bbb339282d3f777089a33f0a391b8/plug-ins/file-psd/psd-layer-res-load.c#L1345-L1438

use std::collections::{HashMap, HashSet};

use image::RgbaImage;
use koharu_renderer::{
    Composition, LayerKind, RasterOptions, Renderer, TextMetadata as RenderedTextMetadata,
};
use koharu_scene::{TextAlignment, WritingMode};

use crate::{
    engine_data::{TextJustification, TextOrientation},
    error::PsdExportError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextLayerMode {
    Rasterized,
    Editable,
}

#[derive(Debug, Clone)]
pub struct PsdExportOptions {
    pub text_layer_mode: TextLayerMode,
}

impl Default for PsdExportOptions {
    fn default() -> Self {
        Self {
            text_layer_mode: TextLayerMode::Editable,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Document {
    pub width: u32,
    pub height: u32,
    pub merged: RgbaImage,
    pub layers: Vec<Layer>,
}

#[derive(Debug, Clone)]
pub(crate) struct Layer {
    pub id: i32,
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub pixels: RgbaImage,
    pub opacity: u8,
    pub hidden: bool,
    pub text: Option<TextMetadata>,
}

#[derive(Debug, Clone)]
pub(crate) struct TextMetadata {
    pub index: i32,
    pub text: String,
    pub bounds: [f64; 4],
    pub transform: [f64; 6],
    pub orientation: TextOrientation,
    pub justification: TextJustification,
    pub font_index: usize,
    pub font_set: Vec<String>,
    pub font_size: f64,
    pub color: [u8; 4],
    pub box_width: f64,
    pub box_height: f64,
}

pub(crate) async fn build(
    renderer: &Renderer,
    composition: &Composition,
    options: &PsdExportOptions,
) -> Result<Document, PsdExportError> {
    let raster_options = RasterOptions::default();
    let merged = renderer.rasterize(composition, &raster_options).await?;
    let width = merged.image.width();
    let height = merged.image.height();
    validate_dimensions(width, height)?;

    if composition.layers().len() > i16::MAX as usize {
        return Err(PsdExportError::TooManyLayers(composition.layers().len()));
    }
    let text_entities = composition
        .layers()
        .iter()
        .filter_map(|layer| match layer.kind() {
            LayerKind::Text(text) => Some((layer.entity(), text)),
            LayerKind::Pixel(_) => None,
        })
        .filter(|(_, text)| !text.text.trim().is_empty())
        .collect::<Vec<_>>();
    let font_set = collect_fonts(text_entities.iter().map(|(_, text)| *text));
    let mut text_indices = HashMap::with_capacity(text_entities.len());
    for (offset, (entity, _)) in text_entities.iter().enumerate() {
        let index = i32::try_from(offset + 1)
            .map_err(|_| PsdExportError::TooManyLayers(text_entities.len()))?;
        text_indices.insert(*entity, index);
    }

    let mut layers = Vec::with_capacity(composition.layers().len());
    // GIMP writes the application layer list in reverse so PSD records remain bottom-to-top.
    // Reversing the complete authored list preserves text/pixel interleaving.
    for layer in composition.layers().iter().rev() {
        let (name, text) = match layer.kind() {
            LayerKind::Pixel(pixel) => (pixel.name.clone(), None),
            LayerKind::Text(text) if !text.text.trim().is_empty() => {
                let index = text_indices[&layer.entity()];
                (
                    format!("TL {index:03} {}", layer.entity()),
                    match options.text_layer_mode {
                        TextLayerMode::Rasterized => None,
                        TextLayerMode::Editable => Some(text_metadata(index, text, &font_set)),
                    },
                )
            }
            LayerKind::Text(_) => continue,
        };
        let isolated = composition
            .cropped(layer.entity())?
            .ok_or(PsdExportError::MissingRenderedEntity(layer.entity()))?;
        let rendered = renderer.rasterize(&isolated, &raster_options).await?;
        validate_pixels(&name, &rendered.image)?;
        let presentation = layer.presentation();
        layers.push(Layer {
            id: 0,
            name,
            left: rendered.left,
            top: rendered.top,
            pixels: rendered.image,
            opacity: (presentation.opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
            hidden: !presentation.visible,
            text,
        });
    }
    let layer_count = layers.len();
    if layer_count > i16::MAX as usize {
        return Err(PsdExportError::TooManyLayers(layer_count));
    }
    for (offset, layer) in layers.iter_mut().enumerate() {
        layer.id =
            i32::try_from(offset + 1).map_err(|_| PsdExportError::TooManyLayers(layer_count))?;
    }

    Ok(Document {
        width,
        height,
        merged: merged.image,
        layers,
    })
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), PsdExportError> {
    if width == 0 || height == 0 || width > 30_000 || height > 30_000 {
        return Err(PsdExportError::UnsupportedDimensions { width, height });
    }
    Ok(())
}

fn validate_pixels(layer: &str, pixels: &RgbaImage) -> Result<(), PsdExportError> {
    let width = pixels.width() as i32;
    let height = pixels.height() as i32;
    if width <= 0 || height <= 0 {
        return Err(PsdExportError::InvalidLayerBounds {
            layer: layer.to_owned(),
            width,
            height,
        });
    }
    Ok(())
}

fn collect_fonts<'a>(texts: impl Iterator<Item = &'a RenderedTextMetadata>) -> Vec<String> {
    let mut fonts = Vec::new();
    let mut seen = HashSet::new();
    for font in texts.flat_map(|text| &text.post_script_fonts) {
        if seen.insert(font) {
            fonts.push(font.clone());
        }
    }
    fonts
}

fn text_metadata(index: i32, text: &RenderedTextMetadata, font_set: &[String]) -> TextMetadata {
    let angle = f64::from(text.angle_degrees).to_radians();
    let bounds = text.layout_bounds;
    let primary_font = text.post_script_fonts.first();
    let font_index = primary_font
        .and_then(|font| font_set.iter().position(|candidate| candidate == font))
        .unwrap_or(0);
    TextMetadata {
        index,
        text: text.text.clone(),
        bounds: [
            f64::from(bounds.x),
            f64::from(bounds.y),
            f64::from(bounds.x + bounds.width),
            f64::from(bounds.y + bounds.height),
        ],
        transform: [
            angle.cos(),
            angle.sin(),
            -angle.sin(),
            angle.cos(),
            f64::from(bounds.x),
            f64::from(bounds.y),
        ],
        orientation: match text.writing_mode {
            WritingMode::Horizontal => TextOrientation::Horizontal,
            WritingMode::Vertical => TextOrientation::Vertical,
        },
        justification: match text.alignment {
            TextAlignment::Start | TextAlignment::Justify => TextJustification::Left,
            TextAlignment::Center => TextJustification::Center,
            TextAlignment::End => TextJustification::Right,
        },
        font_index,
        font_set: font_set.to_vec(),
        font_size: f64::from(text.font_size),
        color: text.color,
        box_width: f64::from(bounds.width.max(1.0)),
        box_height: f64::from(bounds.height.max(1.0)),
    }
}

#[cfg(test)]
mod tests {
    use koharu_renderer::RenderBounds;

    use super::*;

    fn rendered_text(fonts: &[&str]) -> RenderedTextMetadata {
        RenderedTextMetadata {
            text: "Hello".to_owned(),
            language: None,
            rendered_bounds: RenderBounds {
                x: 10.0,
                y: 20.0,
                width: 80.0,
                height: 24.0,
            },
            layout_bounds: RenderBounds {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 40.0,
            },
            post_script_fonts: fonts.iter().map(|font| (*font).to_owned()).collect(),
            font_size: 24.0,
            color: [1, 2, 3, 255],
            alignment: TextAlignment::Center,
            writing_mode: WritingMode::Horizontal,
            angle_degrees: 0.0,
        }
    }

    #[test]
    fn fonts_keep_first_resolved_order_without_duplicates() {
        let first = rendered_text(&["Primary", "Fallback"]);
        let second = rendered_text(&["Fallback", "Other"]);
        assert_eq!(
            collect_fonts([&first, &second].into_iter()),
            ["Primary", "Fallback", "Other"]
        );
    }

    #[test]
    fn text_metadata_uses_renderer_resolved_presentation() {
        let mut text = rendered_text(&["Primary"]);
        text.writing_mode = WritingMode::Vertical;
        text.alignment = TextAlignment::End;
        text.angle_degrees = 90.0;
        let metadata = text_metadata(3, &text, &["Primary".to_owned()]);
        assert_eq!(metadata.orientation, TextOrientation::Vertical);
        assert_eq!(metadata.justification, TextJustification::Right);
        assert_eq!(metadata.font_size, 24.0);
        assert!((metadata.transform[0]).abs() < 1e-12);
        assert!((metadata.transform[1] - 1.0).abs() < 1e-12);
    }
}
