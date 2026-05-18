//! Bridge between the scene model and `koharu-psd`.
//!
//! Walks the current scene, resolves every blob reference, and feeds the
//! result into `koharu_psd::write_document` one page at a time.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use image::DynamicImage;
use koharu_app::ProjectSession;
use koharu_core::{
    BlobRef, FontPrediction, ImageRole, MaskRole, NodeKind, PageId, Scene, TextAlign, TextData,
    TextDirection, TextStyle,
};
use koharu_psd::{
    PsdBlobRef, PsdDocument, PsdExportOptions, PsdFontPrediction, PsdNamedFontPrediction,
    PsdShaderEffect, PsdTextAlign, PsdTextBlock, PsdTextDirection, PsdTextStyle, ResolvedDocument,
    write_document,
};

/// Resolved page artifacts ready to hand to `koharu-psd`.
struct ResolvedPage {
    doc: PsdDocument,
    source: DynamicImage,
    segment: Option<DynamicImage>,
    inpainted: Option<DynamicImage>,
    rendered: Option<DynamicImage>,
    brush: Option<DynamicImage>,
    block_images: HashMap<PsdBlobRef, DynamicImage>,
}

/// Encode a single page as PSD bytes.
pub fn psd_bytes_for_page(
    session: &Arc<ProjectSession>,
    renderer: &koharu_app::renderer::Renderer,
    default_font_override: Option<String>,
    page_id: PageId,
) -> Result<Vec<u8>> {
    let scene: Scene = session.scene_snapshot();
    let page = scene
        .pages
        .get(&page_id)
        .ok_or_else(|| anyhow::anyhow!("page {page_id} not found"))?;
    let project_default_font = scene.project.style.default_font.clone();
    let ResolvedPage {
        doc,
        source,
        segment,
        inpainted,
        rendered,
        brush,
        block_images,
    } = resolve_page_blobs(
        session,
        renderer,
        default_font_override,
        project_default_font,
        page,
    )
    .with_context(|| format!("page {page_id}"))?;
    let resolved = ResolvedDocument {
        document: &doc,
        source: &source,
        segment: segment.as_ref(),
        inpainted: inpainted.as_ref(),
        rendered: rendered.as_ref(),
        brush_layer: brush.as_ref(),
        block_images: &block_images,
    };
    let opts = PsdExportOptions::default();
    let mut buf = Vec::new();
    write_document(&mut buf, &resolved, &opts).map_err(anyhow::Error::new)?;
    Ok(buf)
}

fn resolve_page_blobs(
    session: &ProjectSession,
    renderer: &koharu_app::renderer::Renderer,
    default_font_override: Option<String>,
    project_default_font: Option<String>,
    page: &koharu_core::Page,
) -> Result<ResolvedPage> {
    let mut source: Option<DynamicImage> = None;
    let mut segment: Option<DynamicImage> = None;
    let mut inpainted: Option<DynamicImage> = None;
    let mut rendered: Option<DynamicImage> = None;
    let mut brush: Option<DynamicImage> = None;
    let mut block_images: HashMap<PsdBlobRef, DynamicImage> = HashMap::new();
    let mut text_blocks: Vec<PsdTextBlock> = Vec::new();
    let mut fonts = vec!["AdobeInvisFont".to_string()];
    let mut font_map: HashMap<String, usize> = HashMap::new();
    font_map.insert("AdobeInvisFont".to_string(), 0);

    for (node_id, node) in &page.nodes {
        match &node.kind {
            NodeKind::Image(img) => {
                let decoded = session.blobs.load_image(&img.blob)?;
                match img.role {
                    ImageRole::Source => source = Some(decoded),
                    ImageRole::Inpainted => inpainted = Some(decoded),
                    ImageRole::Rendered => rendered = Some(decoded),
                    ImageRole::Custom => {
                        // Custom layers rendered as-is on top of inpainted; they
                        // don't have a dedicated PSD slot in the current export,
                        // so they land in the final composite only.
                    }
                }
            }
            NodeKind::Mask(mask) => {
                let decoded = session.blobs.load_image(&mask.blob)?;
                match mask.role {
                    MaskRole::Segment => segment = Some(decoded),
                    MaskRole::BrushInpaint => brush = Some(decoded),
                    // Bubble mask is a render-time aid (grows text bboxes
                    // inside speech balloons) — not exported as its own
                    // PSD layer.
                    MaskRole::Bubble => {}
                }
            }
            NodeKind::Text(text) => {
                if let Some(sprite) = text.sprite.as_ref() {
                    let decoded = session.blobs.load_image(sprite)?;
                    block_images.insert(blob_ref_to_psd(sprite), decoded);
                }

                let style = resolve_export_font_style(
                    text,
                    &project_default_font,
                    default_font_override.as_deref(),
                );

                tracing::debug!(
                    "Resolving font for text node {}: families={:?}, default={:?}, override={:?}",
                    node_id,
                    text.style.as_ref().map(|s| &s.font_families),
                    project_default_font,
                    default_font_override
                );
                let ps_name = renderer
                    .resolve_post_script_name(&style, text.translation.as_deref())
                    .unwrap_or_else(|_| "ArialMT".to_string());
                let font_index = *font_map.entry(ps_name.clone()).or_insert_with(|| {
                    let idx = fonts.len();
                    fonts.push(ps_name);
                    idx
                });

                text_blocks.push(text_to_psd(
                    node_id,
                    &node.transform,
                    text,
                    Some(font_index),
                ));
            }
        }
    }

    let source = source.ok_or_else(|| anyhow::anyhow!("page has no source image"))?;
    let doc = PsdDocument {
        width: page.width,
        height: page.height,
        text_blocks,
        fonts,
    };
    Ok(ResolvedPage {
        doc,
        source,
        segment,
        inpainted,
        rendered,
        brush,
        block_images,
    })
}

/// Encode a single page's image for `role` as PNG bytes. Returns `None` if
/// the page doesn't have that role layer. Used by `Rendered` / `Inpainted`
/// export formats.
pub fn png_bytes_for_page(
    session: &Arc<ProjectSession>,
    page_id: PageId,
    role: ImageRole,
) -> Result<Option<Vec<u8>>> {
    let scene: Scene = session.scene_snapshot();
    let page = scene
        .pages
        .get(&page_id)
        .ok_or_else(|| anyhow::anyhow!("page {page_id} not found"))?;

    let blob = page.nodes.values().find_map(|n| match &n.kind {
        NodeKind::Image(img) if img.role == role => Some(img.blob.clone()),
        _ => None,
    });
    let Some(blob_ref) = blob else {
        return Ok(None);
    };
    let image = session.blobs.load_image(&blob_ref)?;
    let mut buf: Vec<u8> = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(anyhow::Error::new)?;
    Ok(Some(buf))
}

fn resolve_export_font_style(
    text: &TextData,
    project_default: &Option<String>,
    ui_override: Option<&str>,
) -> TextStyle {
    let mut style = text.style.clone().unwrap_or_default();
    let default_font = ui_override
        .map(|s| s.to_string())
        .or_else(|| project_default.clone());

    // If we have a global default, and the current style is either empty
    // or looks like a raw AI path prediction, prioritize the global default.
    let is_ai_path = style
        .font_families
        .first()
        .map(|f| {
            let lower = f.to_ascii_lowercase();
            lower.ends_with(".otf")
                || lower.ends_with(".ttf")
                || lower.ends_with(".ttc")
                || lower.ends_with(".woff")
                || lower.ends_with(".woff2")
        })
        .unwrap_or(false);

    if (style.font_families.is_empty() || is_ai_path)
        && let Some(df) = default_font
    {
        // Move the user's preference to the front
        style.font_families.insert(0, df);
    }

    style
}

fn text_to_psd(
    node_id: &koharu_core::NodeId,
    transform: &koharu_core::Transform,
    text: &TextData,
    font_index: Option<usize>,
) -> PsdTextBlock {
    let layer_transform = match (&text.sprite, &text.sprite_transform) {
        (Some(_), Some(sprite_transform)) => sprite_transform,
        _ => transform,
    };

    PsdTextBlock {
        id: node_id.to_string(),
        x: layer_transform.x,
        y: layer_transform.y,
        width: layer_transform.width,
        height: layer_transform.height,
        translation: text.translation.clone(),
        style: text.style.as_ref().map(convert_style),
        rendered: text.sprite.as_ref().map(blob_ref_to_psd),
        rotation_deg: text.rotation_deg,
        font_prediction: text.font_prediction.as_ref().map(convert_prediction),
        source_direction: text.source_direction.map(convert_dir),
        rendered_direction: text.rendered_direction.map(convert_dir),
        detected_font_size_px: text.detected_font_size_px,
        font_index,
    }
}

fn convert_style(s: &TextStyle) -> PsdTextStyle {
    PsdTextStyle {
        font_families: s.font_families.clone(),
        font_size: s.font_size,
        color: s.color,
        effect: s.effect.map(|e| PsdShaderEffect {
            italic: e.italic,
            bold: e.bold,
        }),
        text_align: s.text_align.map(convert_align),
    }
}

fn convert_align(a: TextAlign) -> PsdTextAlign {
    match a {
        TextAlign::Left => PsdTextAlign::Left,
        TextAlign::Center => PsdTextAlign::Center,
        TextAlign::Right => PsdTextAlign::Right,
    }
}

fn convert_dir(d: TextDirection) -> PsdTextDirection {
    match d {
        TextDirection::Horizontal => PsdTextDirection::Horizontal,
        TextDirection::Vertical => PsdTextDirection::Vertical,
    }
}

fn convert_prediction(p: &FontPrediction) -> PsdFontPrediction {
    PsdFontPrediction {
        named_fonts: p
            .named_fonts
            .iter()
            .map(|n| PsdNamedFontPrediction {
                name: n.name.clone(),
            })
            .collect(),
        text_color: p.text_color,
        font_size_px: p.font_size_px,
        angle_deg: p.angle_deg,
    }
}

fn blob_ref_to_psd(r: &BlobRef) -> PsdBlobRef {
    PsdBlobRef::new(r.hash())
}

#[cfg(test)]
mod tests {
    use koharu_core::{BlobRef, NodeId, TextData, Transform};

    use super::text_to_psd;

    #[test]
    fn text_to_psd_uses_sprite_transform_for_rendered_text_layer() {
        let node_transform = Transform {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
            rotation_deg: 0.0,
        };
        let sprite_transform = Transform {
            x: 100.0,
            y: 200.0,
            width: 300.0,
            height: 400.0,
            rotation_deg: 0.0,
        };
        let text = TextData {
            translation: Some("Hello".to_string()),
            sprite: Some(BlobRef::new("sprite")),
            sprite_transform: Some(sprite_transform),
            ..Default::default()
        };

        let block = text_to_psd(&NodeId::new(), &node_transform, &text, Some(0));

        assert_eq!(block.x, sprite_transform.x);
        assert_eq!(block.y, sprite_transform.y);
        assert_eq!(block.width, sprite_transform.width);
        assert_eq!(block.height, sprite_transform.height);
    }

    #[test]
    fn text_to_psd_ignores_stale_sprite_transform_without_sprite() {
        let node_transform = Transform {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
            rotation_deg: 0.0,
        };
        let text = TextData {
            translation: Some("Hello".to_string()),
            sprite_transform: Some(Transform {
                x: 100.0,
                y: 200.0,
                width: 300.0,
                height: 400.0,
                rotation_deg: 0.0,
            }),
            ..Default::default()
        };

        let block = text_to_psd(&NodeId::new(), &node_transform, &text, Some(0));

        assert_eq!(block.x, node_transform.x);
        assert_eq!(block.y, node_transform.y);
        assert_eq!(block.width, node_transform.width);
        assert_eq!(block.height, node_transform.height);
    }
}
