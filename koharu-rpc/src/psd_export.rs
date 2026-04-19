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
pub fn psd_bytes_for_page(session: &Arc<ProjectSession>, page_id: PageId) -> Result<Vec<u8>> {
    let scene: Scene = session.scene_snapshot();
    let page = scene
        .pages
        .get(&page_id)
        .ok_or_else(|| anyhow::anyhow!("page {page_id} not found"))?;
    let ResolvedPage {
        doc,
        source,
        segment,
        inpainted,
        rendered,
        brush,
        block_images,
    } = resolve_page_blobs(session, page).with_context(|| format!("page {page_id}"))?;
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

fn resolve_page_blobs(session: &ProjectSession, page: &koharu_core::Page) -> Result<ResolvedPage> {
    let mut source: Option<DynamicImage> = None;
    let mut segment: Option<DynamicImage> = None;
    let mut inpainted: Option<DynamicImage> = None;
    let mut rendered: Option<DynamicImage> = None;
    let mut brush: Option<DynamicImage> = None;
    let mut block_images: HashMap<PsdBlobRef, DynamicImage> = HashMap::new();
    let mut text_blocks: Vec<PsdTextBlock> = Vec::new();

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
                text_blocks.push(text_to_psd(node_id, &node.transform, text));
            }
        }
    }

    let source = source.ok_or_else(|| anyhow::anyhow!("page has no source image"))?;
    let doc = PsdDocument {
        width: page.width,
        height: page.height,
        text_blocks,
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

fn text_to_psd(
    node_id: &koharu_core::NodeId,
    transform: &koharu_core::Transform,
    text: &TextData,
) -> PsdTextBlock {
    PsdTextBlock {
        id: node_id.to_string(),
        x: transform.x,
        y: transform.y,
        width: transform.width,
        height: transform.height,
        translation: text.translation.clone(),
        style: text.style.as_ref().map(convert_style),
        rendered: text.sprite.as_ref().map(blob_ref_to_psd),
        rotation_deg: text.rotation_deg,
        font_prediction: text.font_prediction.as_ref().map(convert_prediction),
        source_direction: text.source_direction.map(convert_dir),
        rendered_direction: text.rendered_direction.map(convert_dir),
        detected_font_size_px: text.detected_font_size_px,
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
