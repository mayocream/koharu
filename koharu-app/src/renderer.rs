use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use image::{DynamicImage, imageops};
use koharu_core::{
    BlobRef, FontFaceInfo, FontSource, TextBlock, TextShaderEffect, TextStrokeStyle, TextStyle,
};

use koharu_renderer::{
    font::{FaceInfo, Font, FontBook},
    layout::{LayoutRun, TextLayout, WritingMode},
    renderer::{RenderOptions, RenderStrokeOptions, TinySkiaRenderer},
    text::{
        latin::{LayoutBox, layout_box_from_block},
        script::{
            font_families_for_text, normalize_translation_for_layout, writing_mode_for_block,
        },
    },
};

use crate::google_fonts::GoogleFontService;
use crate::storage;

/// Grouped options for [`Renderer::render_to_blob`] to keep the arg count manageable.
pub struct RenderTextOptions<'a> {
    pub text_block_index: Option<usize>,
    pub shader_effect: TextShaderEffect,
    pub shader_stroke: Option<TextStrokeStyle>,
    pub document_font: Option<&'a str>,
    pub bubbles: &'a [koharu_core::BubbleRegion],
    pub image_width: u32,
    pub image_height: u32,
}

// ---------------------------------------------------------------------------
// Font size calculation
// ---------------------------------------------------------------------------

/// Minimum font size based on image dimensions.
/// Assumes ~150 PPI for manga: a 1800px wide page ≈ 12in, min readable ≈ 8pt ≈ 11px.
fn min_font_size_for_image(image_width: u32, image_height: u32) -> f32 {
    let max_dim = image_width.max(image_height) as f32;
    // Scale: 1800px → 11px min, 3600px → 14px min, 900px → 8px min
    (max_dim / 160.0).clamp(6.0, 20.0)
}

/// Get the detected font size for a text block (from font prediction or block metadata).
fn detected_font_size(block: &TextBlock) -> Option<f32> {
    block
        .font_prediction
        .as_ref()
        .map(|fp| fp.font_size_px)
        .or(block.detected_font_size_px)
        .filter(|&s| s > 0.0)
}

/// Cluster similar font sizes (within ±20%) and average each cluster.
/// Returns a vec of averaged sizes, one per input block (in the same order).
fn cluster_font_sizes(blocks: &[TextBlock]) -> Vec<Option<f32>> {
    let sizes: Vec<Option<f32>> = blocks.iter().map(detected_font_size).collect();

    // Collect (index, size) for blocks with a detected size.
    let mut with_size: Vec<(usize, f32)> = sizes
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.map(|s| (i, s)))
        .collect();
    with_size.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut result = sizes.clone();

    // Greedy clustering: walk sorted sizes, group within ±20%.
    let mut used = vec![false; with_size.len()];
    for i in 0..with_size.len() {
        if used[i] {
            continue;
        }
        let mut cluster_sum = with_size[i].1;
        let mut cluster_indices = vec![with_size[i].0];
        used[i] = true;

        for j in (i + 1)..with_size.len() {
            if used[j] {
                continue;
            }
            // Within ±20% of the cluster's first element
            if (with_size[j].1 - with_size[i].1).abs() / with_size[i].1 <= 0.2 {
                cluster_sum += with_size[j].1;
                cluster_indices.push(with_size[j].0);
                used[j] = true;
            }
        }

        let avg = cluster_sum / cluster_indices.len() as f32;
        for &idx in &cluster_indices {
            result[idx] = Some(avg);
        }
    }

    result
}

/// Calculate the font size for a block given a constraint box and text.
/// Starts at `base_size`, shrinks if text doesn't fit, clamps at `min_size`.
fn fit_font_size<'a>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    constraint_width: f32,
    constraint_height: f32,
    base_size: f32,
    min_size: f32,
) -> Result<LayoutRun<'a>> {
    // Try the base size first.
    let mut size = base_size.round().max(min_size);
    loop {
        let layout = layout_builder
            .clone()
            .with_font_size(size)
            .with_max_width(constraint_width)
            .with_max_height(constraint_height)
            .run(text)?;
        if layout.width <= constraint_width && layout.height <= constraint_height {
            return Ok(layout);
        }
        if size <= min_size {
            // At min size, allow overflow.
            return Ok(layout);
        }
        // Shrink by 1px and retry.
        size = (size - 1.0).max(min_size);
    }
}

pub struct Renderer {
    fontbook: Arc<Mutex<FontBook>>,
    renderer: TinySkiaRenderer,
    symbol_fallbacks: Vec<Font>,
    pub google_fonts: Arc<GoogleFontService>,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        let mut fontbook = FontBook::new();
        let symbol_fallbacks = load_symbol_fallbacks(&mut fontbook);
        let app_data_root = koharu_runtime::default_app_data_root();
        let google_fonts = Arc::new(
            GoogleFontService::new(&app_data_root)
                .context("failed to initialize Google Fonts service")?,
        );
        Ok(Self {
            fontbook: Arc::new(Mutex::new(fontbook)),
            renderer: TinySkiaRenderer::new()?,
            symbol_fallbacks,
            google_fonts,
        })
    }

    pub fn available_fonts(&self) -> Result<Vec<FontFaceInfo>> {
        let fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock fontbook"))?;
        let mut seen = std::collections::HashSet::new();
        let mut fonts = fontbook
            .all_families()
            .into_iter()
            .filter(|face| !face.post_script_name.is_empty())
            .filter_map(|face| {
                let family_name = face
                    .families
                    .first()
                    .map(|(family, _)| family.clone())
                    .unwrap_or_else(|| face.post_script_name.clone());
                if seen.insert(family_name.clone()) {
                    Some(FontFaceInfo {
                        family_name,
                        post_script_name: face.post_script_name,
                        source: FontSource::System,
                        category: None,
                        cached: true,
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // Google Fonts (from catalog)
        let catalog = self.google_fonts.catalog();
        for entry in &catalog.fonts {
            if seen.insert(entry.family.clone()) {
                fonts.push(FontFaceInfo {
                    family_name: entry.family.clone(),
                    post_script_name: entry.family.clone(),
                    source: FontSource::Google,
                    category: Some(entry.category.clone()),
                    cached: false,
                });
            }
        }

        fonts.sort();
        Ok(fonts)
    }

    /// Render text blocks and optionally compose the full rendered image.
    /// Returns an optional BlobRef for the full rendered composite.
    /// Text block `rendered` fields are updated in-place with BlobRefs to per-block images.
    #[tracing::instrument(level = "info", skip_all)]
    pub fn render_to_blob(
        &self,
        images: &storage::ImageCache,
        _source_img: &DynamicImage,
        inpainted_img: Option<&DynamicImage>,
        brush_layer_img: Option<&DynamicImage>,
        text_blocks: &mut [TextBlock],
        opts: RenderTextOptions<'_>,
    ) -> Result<Option<BlobRef>> {
        // Compute clustered font sizes across all blocks for consistency.
        let clustered_sizes = cluster_font_sizes(text_blocks);
        let min_font = min_font_size_for_image(opts.image_width, opts.image_height);

        let mut blocks_to_render: Vec<&mut TextBlock> = match opts.text_block_index {
            Some(index) => text_blocks
                .get_mut(index)
                .map(|tb| vec![tb])
                .ok_or_else(|| anyhow::anyhow!("Text block index out of bounds"))?,
            None => text_blocks.iter_mut().collect(),
        };

        // Map block indices to their clustered font sizes.
        let render_font_sizes: Vec<Option<f32>> = match opts.text_block_index {
            Some(index) => vec![clustered_sizes.get(index).copied().flatten()],
            None => clustered_sizes,
        };

        // Bubble expansion disabled — causes text to fly away from block position.
        // Bubbles are used for font size estimation via detection, not layout.
        let render_areas: Vec<Option<LayoutBox>> = vec![None; blocks_to_render.len()];

        let block_renders: Vec<Option<(DynamicImage, usize)>> = {
            use rayon::prelude::*;
            let span = tracing::info_span!("render_blocks", count = blocks_to_render.len());
            let _s = span.enter();
            let parent_span = span.clone();
            blocks_to_render
                .par_iter_mut()
                .enumerate()
                .map(|(i, text_block)| {
                    let _guard = parent_span.enter();
                    let base_font_size = render_font_sizes.get(i).copied().flatten();
                    let bubble_area = render_areas.get(i).copied().flatten();
                    match self.render_text_block(
                        text_block,
                        opts.shader_effect,
                        opts.shader_stroke.clone(),
                        opts.document_font,
                        bubble_area,
                        base_font_size,
                        min_font,
                    ) {
                        Ok(Some(rendered_img)) => Some((rendered_img, i)),
                        Ok(None) => None,
                        Err(e) => {
                            tracing::warn!("Failed to render text block: {e}");
                            None
                        }
                    }
                })
                .collect()
        };

        {
            let _s = tracing::info_span!("store_block_blobs").entered();
            for (img, idx) in block_renders.iter().flatten() {
                let blob_ref = images.store_raw(img)?;
                blocks_to_render[*idx].rendered = Some(blob_ref);
            }
        }

        if let Some(inpainted) = inpainted_img
            && opts.text_block_index.is_none()
        {
            let _s = tracing::info_span!("compose").entered();
            let mut rendered = inpainted.to_rgba8();

            if let Some(brush_layer) = brush_layer_img {
                let brush = brush_layer.to_rgba8();
                imageops::overlay(&mut rendered, &brush, 0, 0);
            }

            for text_block in &blocks_to_render {
                let Some(ref blob_ref) = text_block.rendered else {
                    continue;
                };
                let block_img = images.load(blob_ref)?;
                let rx = text_block.render_x.unwrap_or(text_block.x);
                let ry = text_block.render_y.unwrap_or(text_block.y);
                imageops::overlay(&mut rendered, &block_img, rx as i64, ry as i64);
            }
            let rendered_ref = images.store_raw(&DynamicImage::from(rendered))?;
            return Ok(Some(rendered_ref));
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_text_block(
        &self,
        text_block: &mut TextBlock,
        effect: TextShaderEffect,
        global_stroke: Option<TextStrokeStyle>,
        document_font: Option<&str>,
        bubble_area: Option<LayoutBox>,
        base_font_size: Option<f32>,
        min_font_size: f32,
    ) -> Result<Option<DynamicImage>> {
        let Some(translation) = text_block.translation.as_ref().cloned() else {
            return Ok(None);
        };
        if translation.is_empty() {
            return Ok(None);
        };
        let normalized_translation = normalize_translation_for_layout(&translation);
        let layout_source_block = TextBlock {
            x: text_block.x,
            y: text_block.y,
            width: text_block.width.max(1.0),
            height: text_block.height.max(1.0),
            translation: Some(translation.clone()),
            source_direction: text_block.source_direction,
            rendered_direction: text_block.rendered_direction,
            ..Default::default()
        };

        let mut style = text_block.style.clone().unwrap_or_else(|| TextStyle {
            font_families: Vec::new(),
            font_size: None,
            color: [0, 0, 0, 255],
            effect: None,
            stroke: None,
            text_align: None,
        });

        // Font cascade: per-block → document default → script fallback
        if style.font_families.is_empty()
            && let Some(font) = document_font
        {
            style.font_families.push(font.to_string());
        }
        apply_default_font_families(&mut style.font_families, &normalized_translation);
        let font = {
            let _s = tracing::info_span!("select_font").entered();
            self.select_font(&style)?
        };
        let block_effect = style.effect.unwrap_or(effect);
        let color = text_block
            .style
            .as_ref()
            .map(|style| style.color)
            .or_else(|| {
                text_block.font_prediction.as_ref().map(|pred| {
                    [
                        pred.text_color[0],
                        pred.text_color[1],
                        pred.text_color[2],
                        255,
                    ]
                })
            })
            .unwrap_or([0, 0, 0, 255]);
        let writing_mode = writing_mode_for_block(&layout_source_block);
        let text_align = style.text_align;
        let block_box = layout_box_from_block(&layout_source_block);
        let (layout_box, bubble_expanded) = match bubble_area {
            Some(area) => (area, true),
            None => (block_box, false),
        };

        // Determine base font size: user-set > clustered detected > fallback.
        let effective_base_size = style.font_size.or(base_font_size).unwrap_or_else(|| {
            // Fallback: estimate from layout box height.
            (layout_box.height * 0.3).clamp(min_font_size, 60.0)
        });

        let mut layout_builder = TextLayout::new(&font, None)
            .with_fallback_fonts(&self.symbol_fallbacks)
            .with_writing_mode(writing_mode);

        if let Some(align) = text_align {
            layout_builder = layout_builder.with_alignment(align);
        }

        let layout = {
            let _s = tracing::info_span!("layout").entered();
            fit_font_size(
                &layout_builder,
                &normalized_translation,
                layout_box.width,
                layout_box.height,
                effective_base_size,
                min_font_size,
            )?
        };

        let resolved_stroke = resolve_stroke_style(
            text_block,
            style.stroke.as_ref(),
            global_stroke.as_ref(),
            layout.font_size,
            color,
        );
        let rendered = {
            let _s = tracing::info_span!("rasterize").entered();
            self.renderer.render(
                &layout,
                writing_mode,
                &RenderOptions {
                    font_size: layout.font_size,
                    color,
                    effect: block_effect,
                    stroke: resolved_stroke,
                    ..Default::default()
                },
            )?
        };

        text_block.rendered_direction = Some(match writing_mode {
            WritingMode::Horizontal => koharu_core::TextDirection::Horizontal,
            WritingMode::VerticalRl => koharu_core::TextDirection::Vertical,
        });
        // Store actual render area when bubble expansion was used.
        if bubble_expanded {
            text_block.render_x = Some(layout_box.x.round());
            text_block.render_y = Some(layout_box.y.round());
            text_block.render_width = Some(layout_box.width.round());
            text_block.render_height = Some(layout_box.height.round());
        } else {
            text_block.render_x = None;
            text_block.render_y = None;
            text_block.render_width = None;
            text_block.render_height = None;
        }
        // rendered field will be set by the caller with a BlobRef
        let _persisted_style = text_block.style.get_or_insert_with(|| TextStyle {
            font_families: Vec::new(),
            font_size: None,
            color,
            effect: None,
            stroke: None,
            text_align: None,
        });
        // font_families is NOT set here — it only contains user-explicit choices
        Ok(Some(DynamicImage::ImageRgba8(rendered)))
    }

    fn select_font(&self, style: &TextStyle) -> Result<Font> {
        let mut fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to lock fontbook"))?;

        // Try each candidate through the full resolution chain before moving
        // to the next.  This ensures a global font (first in the list) is
        // resolved from the disk cache even when a previously-loaded font
        // appears later in the list.
        for candidate in &style.font_families {
            // Already loaded in FontBook (system font or previously loaded Google Font)?
            let faces = fontbook.all_families();
            if let Some(psn) = face_post_script_name(&faces, candidate) {
                return fontbook.query(&psn);
            }

            // Try Google Fonts disk cache
            if let Some(data) = self.google_fonts.read_cached_file(candidate)? {
                let font = fontbook.load_from_bytes(data)?;
                return Ok(font);
            }
        }

        Err(anyhow::anyhow!(
            "no font found for candidates: {:?}",
            style.font_families
        ))
    }
}

fn default_stroke_width(font_size: f32) -> f32 {
    (font_size * 0.10).clamp(1.2, 8.0)
}

fn apply_default_font_families(font_families: &mut Vec<String>, text: &str) {
    if font_families.is_empty() {
        *font_families = font_families_for_text(text);
    }
}

fn contrasting_stroke_color(text_color: [u8; 4]) -> [u8; 4] {
    let luminance =
        0.299 * text_color[0] as f32 + 0.587 * text_color[1] as f32 + 0.114 * text_color[2] as f32;
    if luminance > 128.0 {
        [0, 0, 0, 255]
    } else {
        [255, 255, 255, 255]
    }
}

fn resolve_stroke_style(
    block: &TextBlock,
    block_stroke: Option<&TextStrokeStyle>,
    global_stroke: Option<&TextStrokeStyle>,
    font_size: f32,
    text_color: [u8; 4],
) -> Option<RenderStrokeOptions> {
    if let Some(stroke) = block_stroke {
        if !stroke.enabled {
            return None;
        }
        return Some(RenderStrokeOptions {
            color: stroke.color,
            width_px: stroke
                .width_px
                .unwrap_or_else(|| default_stroke_width(font_size)),
        });
    }

    if let Some(stroke) = global_stroke {
        if !stroke.enabled {
            return None;
        }
        return Some(RenderStrokeOptions {
            color: stroke.color,
            width_px: stroke
                .width_px
                .unwrap_or_else(|| default_stroke_width(font_size)),
        });
    }

    let auto_stroke_color = contrasting_stroke_color(text_color);

    if let Some(pred) = &block.font_prediction
        && pred.stroke_width_px > 0.0
    {
        return Some(RenderStrokeOptions {
            color: auto_stroke_color,
            width_px: pred.stroke_width_px,
        });
    }

    Some(RenderStrokeOptions {
        color: auto_stroke_color,
        width_px: default_stroke_width(font_size),
    })
}

fn load_symbol_fallbacks(fontbook: &mut FontBook) -> Vec<Font> {
    let candidates = [
        "Segoe UI Symbol",
        "Segoe UI Emoji",
        "Noto Sans Symbols",
        "Noto Sans Symbols2",
        "Noto Color Emoji",
        "Apple Color Emoji",
        "Apple Symbols",
        "Symbola",
        "Arial Unicode MS",
    ];
    let faces = fontbook.all_families();
    candidates
        .iter()
        .filter_map(|candidate| face_post_script_name(&faces, candidate))
        .filter_map(|post_script_name| fontbook.query(&post_script_name).ok())
        .collect()
}

fn face_post_script_name(faces: &[FaceInfo], candidate: &str) -> Option<String> {
    faces
        .iter()
        .find(|face| {
            face.post_script_name == candidate
                || face
                    .families
                    .iter()
                    .any(|(family, _)| family.as_str() == candidate)
        })
        .map(|face| face.post_script_name.clone())
        .filter(|post_script_name| !post_script_name.is_empty())
}

#[allow(dead_code)]
fn compute_render_areas(
    blocks: &[&mut TextBlock],
    bubbles: &[koharu_core::BubbleRegion],
) -> Vec<Option<LayoutBox>> {
    // First pass: compute bubble-expanded area for each block.
    let mut areas: Vec<Option<LayoutBox>> = blocks
        .iter()
        .map(|block| {
            if block.lock_layout_box {
                return None;
            }
            let bubble = find_best_bubble(block, bubbles)?;
            let pad_x = bubble.width * 0.05;
            let pad_y = bubble.height * 0.05;
            Some(LayoutBox {
                x: (bubble.x + pad_x).round(),
                y: (bubble.y + pad_y).round(),
                width: (bubble.width - pad_x * 2.0).round().max(1.0),
                height: (bubble.height - pad_y * 2.0).round().max(1.0),
            })
        })
        .collect();

    // Second pass: if any two expanded areas overlap, fall back both to block dims.
    for i in 0..areas.len() {
        for j in (i + 1)..areas.len() {
            let (Some(a), Some(b)) = (areas[i], areas[j]) else {
                continue;
            };
            let overlap_x = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
            let overlap_y = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
            if overlap_x > 0.0 && overlap_y > 0.0 {
                areas[i] = None;
                areas[j] = None;
                break;
            }
        }
    }

    areas
}

#[allow(dead_code)]
fn find_best_bubble(block: &TextBlock, bubbles: &[koharu_core::BubbleRegion]) -> Option<LayoutBox> {
    if bubbles.is_empty() {
        return None;
    }
    // Block center must be inside the bubble.
    let cx = block.x + block.width * 0.5;
    let cy = block.y + block.height * 0.5;
    let block_area = (block.width * block.height).max(1.0);
    let mut best: Option<(f32, &koharu_core::BubbleRegion)> = None;
    for bubble in bubbles {
        // Check containment: block center inside bubble.
        if cx < bubble.x
            || cx > bubble.x + bubble.width
            || cy < bubble.y
            || cy > bubble.y + bubble.height
        {
            continue;
        }
        let bubble_area = (bubble.width * bubble.height).max(1.0);
        let ix0 = block.x.max(bubble.x);
        let iy0 = block.y.max(bubble.y);
        let ix1 = (block.x + block.width).min(bubble.x + bubble.width);
        let iy1 = (block.y + block.height).min(bubble.y + bubble.height);
        let inter = (ix1 - ix0).max(0.0) * (iy1 - iy0).max(0.0);
        let union = block_area + bubble_area - inter;
        let iou = if union > 0.0 { inter / union } else { 0.0 };
        match &best {
            Some((best_iou, _)) if iou > *best_iou => best = Some((iou, bubble)),
            None if iou > 0.0 => best = Some((iou, bubble)),
            _ => {}
        }
    }
    best.filter(|(iou, _)| *iou > 0.1).map(|(_, b)| LayoutBox {
        x: b.x,
        y: b.y,
        width: b.width,
        height: b.height,
    })
}

#[cfg(test)]
mod tests {
    use super::{apply_default_font_families, resolve_stroke_style};
    use koharu_core::{FontPrediction, TextBlock, TextStrokeStyle};

    #[test]
    fn default_font_families_should_fill_empty_list() {
        let mut font_families = Vec::new();
        apply_default_font_families(&mut font_families, "hello");
        assert!(!font_families.is_empty());
    }

    #[test]
    fn default_stroke_color_uses_black_for_light_text() {
        let stroke = resolve_stroke_style(
            &TextBlock::default(),
            None,
            None,
            16.0,
            [255, 255, 255, 255],
        )
        .expect("default stroke should be present");

        assert_eq!(stroke.color, [0, 0, 0, 255]);
        assert_eq!(stroke.width_px, 1.6);
    }

    #[test]
    fn predicted_stroke_width_keeps_auto_black_or_white_color() {
        let block = TextBlock {
            font_prediction: Some(FontPrediction {
                stroke_color: [12, 34, 56],
                stroke_width_px: 3.0,
                ..Default::default()
            }),
            ..Default::default()
        };

        let stroke = resolve_stroke_style(&block, None, None, 18.0, [255, 255, 255, 255])
            .expect("predicted stroke should be present");

        assert_eq!(stroke.color, [0, 0, 0, 255]);
        assert_eq!(stroke.width_px, 3.0);
    }

    #[test]
    fn explicit_block_stroke_color_is_preserved_even_if_it_matches_text() {
        let stroke = resolve_stroke_style(
            &TextBlock::default(),
            Some(&TextStrokeStyle {
                enabled: true,
                color: [255, 255, 255, 255],
                width_px: Some(2.0),
            }),
            None,
            18.0,
            [255, 255, 255, 255],
        )
        .expect("explicit stroke should be present");

        assert_eq!(stroke.color, [255, 255, 255, 255]);
        assert_eq!(stroke.width_px, 2.0);
    }

    #[test]
    fn explicit_global_stroke_color_is_preserved_even_if_it_matches_text() {
        let stroke = resolve_stroke_style(
            &TextBlock::default(),
            None,
            Some(&TextStrokeStyle {
                enabled: true,
                color: [0, 0, 0, 255],
                width_px: Some(2.0),
            }),
            18.0,
            [0, 0, 0, 255],
        )
        .expect("explicit global stroke should be present");

        assert_eq!(stroke.color, [0, 0, 0, 255]);
        assert_eq!(stroke.width_px, 2.0);
    }
}
