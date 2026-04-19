//! Koharu text renderer.
//!
//! Owns the font book, symbol fallbacks, and Google Fonts service. Exposes
//! [`Renderer::render_page`], which rasterises each text block's translation
//! into an RGBA sprite and composites them onto the inpainted plane.
//!
//! Pure output: the pipeline engine ([`crate::pipeline::engines::renderer`])
//! takes a `RenderOutput` and translates sprites + final composite into ops.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use image::{DynamicImage, imageops};
use koharu_core::{
    FontFaceInfo, FontPrediction, FontSource, NodeId, TextDirection, TextShaderEffect,
    TextStrokeStyle, TextStyle, Transform,
};

use koharu_renderer::{
    TextAlign as RendererTextAlign, TextShaderEffect as RendererEffect,
    font::{FaceInfo, Font, FontBook},
    layout::{LayoutRun, TextLayout, WritingMode},
    renderer::{RenderOptions, RenderStrokeOptions, TinySkiaRenderer},
    text::{
        latin::{BubbleIndex, LayoutBox, layout_box_from_block},
        script::{
            font_families_for_text, normalize_translation_for_layout, writing_mode_for_block,
        },
    },
    types::RenderBlock,
};

use crate::google_fonts::GoogleFontService;

// ---------------------------------------------------------------------------
// Inputs / outputs
// ---------------------------------------------------------------------------

/// Per-block input (immutable snapshot of a scene text node).
#[derive(Debug, Clone)]
pub struct RenderBlockInput {
    pub node_id: NodeId,
    pub transform: Transform,
    pub translation: String,
    pub style: Option<TextStyle>,
    pub font_prediction: Option<FontPrediction>,
    pub source_direction: Option<TextDirection>,
    pub rendered_direction: Option<TextDirection>,
    pub lock_layout_box: bool,
}

/// Document-level render options (shared across all blocks).
#[derive(Debug, Clone, Default)]
pub struct PageRenderOptions {
    pub shader_effect: TextShaderEffect,
    pub shader_stroke: Option<TextStrokeStyle>,
    pub document_font: Option<String>,
}

/// Per-block sprite output. `transform` becomes `TextData.sprite_transform`
/// when the renderer expanded the layout beyond the original bubble.
pub struct RenderedBlock {
    pub node_id: NodeId,
    pub sprite: DynamicImage,
    pub rendered_direction: TextDirection,
    pub expanded_transform: Option<Transform>,
}

/// Result of rendering a whole page.
pub struct RenderOutput {
    pub final_render: DynamicImage,
    pub blocks: Vec<RenderedBlock>,
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

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

    /// List system + cached Google Fonts for the API.
    pub fn available_fonts(&self) -> Result<Vec<FontFaceInfo>> {
        let fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("failed to lock fontbook"))?;
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
                seen.insert(family_name.clone()).then_some(FontFaceInfo {
                    family_name,
                    post_script_name: face.post_script_name,
                    source: FontSource::System,
                    category: None,
                    cached: true,
                })
            })
            .collect::<Vec<_>>();
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

    /// Render every block's translation, composite onto `inpainted`, return
    /// the full page + per-block sprites. Blocks with an empty translation
    /// are skipped (they appear as holes in the composite, falling through to
    /// the inpainted plane).
    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(level = "info", skip_all, fields(blocks = blocks.len()))]
    pub fn render_page(
        &self,
        inpainted: &DynamicImage,
        brush_layer: Option<&DynamicImage>,
        bubble_mask: Option<&DynamicImage>,
        image_width: u32,
        image_height: u32,
        blocks: &[RenderBlockInput],
        opts: &PageRenderOptions,
    ) -> Result<RenderOutput> {
        let min_font = min_font_size_for_image(image_width, image_height);
        // Build the bubble index once per page. The mask encodes each
        // detected bubble as a distinct grayscale ID; the index scans
        // once to record per-ID bboxes and then answers seed→bbox
        // lookups in O(seed_area).
        let bubble_index: Option<BubbleIndex> = bubble_mask.map(|m| BubbleIndex::new(m.to_luma8()));

        let mut rendered_blocks = Vec::with_capacity(blocks.len());
        for block in blocks.iter() {
            match self.render_one(
                block,
                &opts.shader_effect,
                &opts.shader_stroke,
                opts.document_font.as_deref(),
                min_font,
                bubble_index.as_ref(),
            ) {
                Ok(Some(out)) => rendered_blocks.push(out),
                Ok(None) => {}
                Err(e) => tracing::warn!(node = %block.node_id, "render failed: {e:#}"),
            }
        }

        // Compose the final page: inpainted → brush → per-block sprites.
        let mut canvas = inpainted.to_rgba8();
        if let Some(brush) = brush_layer {
            imageops::overlay(&mut canvas, &brush.to_rgba8(), 0, 0);
        }
        for out in &rendered_blocks {
            let (x, y) = placement_origin(find_input(blocks, out.node_id), &out.expanded_transform);
            imageops::overlay(&mut canvas, &out.sprite.to_rgba8(), x as i64, y as i64);
        }
        Ok(RenderOutput {
            final_render: DynamicImage::ImageRgba8(canvas),
            blocks: rendered_blocks,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_one(
        &self,
        block: &RenderBlockInput,
        effect: &TextShaderEffect,
        global_stroke: &Option<TextStrokeStyle>,
        document_font: Option<&str>,
        min_font_size: f32,
        bubble_index: Option<&BubbleIndex>,
    ) -> Result<Option<RenderedBlock>> {
        let translation = block.translation.trim();
        if translation.is_empty() {
            return Ok(None);
        }
        let normalized = normalize_translation_for_layout(translation);

        let layout_source = RenderBlock {
            x: block.transform.x,
            y: block.transform.y,
            width: block.transform.width.max(1.0),
            height: block.transform.height.max(1.0),
            text: translation.to_string(),
        };

        let mut style = block.style.clone().unwrap_or_else(|| TextStyle {
            font_families: Vec::new(),
            font_size: None,
            color: [0, 0, 0, 255],
            effect: None,
            stroke: None,
            text_align: None,
        });
        if style.font_families.is_empty()
            && let Some(font) = document_font
        {
            style.font_families.push(font.to_string());
        }
        apply_default_font_families(&mut style.font_families, &normalized);

        let font = self.select_font(&style)?;
        let block_effect = style.effect.unwrap_or(*effect);
        let color = if style.effect.is_some() {
            style.color
        } else {
            {
                if block.style.is_some() {
                    style.color
                } else if let Some(pred) = &block.font_prediction {
                    [
                        pred.text_color[0],
                        pred.text_color[1],
                        pred.text_color[2],
                        255,
                    ]
                } else {
                    [0, 0, 0, 255]
                }
            }
        };

        let writing_mode = writing_mode_for_block(&layout_source);
        // Translations default to centre alignment inside a bubble — each
        // line sits centred above/below the others, matching manga
        // typesetting convention. Explicit `style.text_align` wins if set.
        let align = style
            .text_align
            .map(core_align_to_renderer)
            .unwrap_or(RendererTextAlign::Center);
        let seed_box = layout_box_from_block(&layout_source);
        let expanded_box = if block.lock_layout_box {
            None
        } else {
            bubble_index.and_then(|idx| idx.lookup(seed_box))
        };
        let layout_box = expanded_box.unwrap_or(seed_box);

        let layout_builder = TextLayout::new(&font, None)
            .with_fallback_fonts(&self.symbol_fallbacks)
            .with_writing_mode(writing_mode)
            .with_alignment(align);
        let max_font = max_font_size_for_box(layout_box, min_font_size);
        let layout = fit_font_size(
            &layout_builder,
            &normalized,
            layout_box.width,
            layout_box.height,
            style.font_size,
            min_font_size,
            max_font,
        )?;

        // A narrow bubble can be narrower than individual words (manga
        // tall-thin balloons frequently are). The layout engine's
        // center-align step skips lines wider than `max_width`, leaving
        // them at x=0 while shorter lines in the same block DO get
        // centered at `max_width/2` — so shorter lines cluster on the
        // left instead of being centred relative to the widest line.
        // Re-run the layout with `max_width = actual_content_width` so
        // every line is centred relative to the block's widest line.
        let layout = if layout.width > layout_box.width + 0.5 {
            layout_builder
                .clone()
                .with_font_size(layout.font_size)
                .with_max_width(layout.width)
                .with_max_height(layout_box.height)
                .run(&normalized)?
        } else {
            layout
        };

        let resolved_stroke = resolve_stroke_style(
            block.font_prediction.as_ref(),
            style.stroke.as_ref(),
            global_stroke.as_ref(),
            layout.font_size,
            color,
        );

        let rendered = self.renderer.render(
            &layout,
            writing_mode,
            &RenderOptions {
                font_size: layout.font_size,
                color,
                effect: shader_core_to_renderer(block_effect),
                stroke: resolved_stroke,
                ..Default::default()
            },
        )?;

        let rendered_direction = match writing_mode {
            WritingMode::Horizontal => TextDirection::Horizontal,
            WritingMode::VerticalRl => TextDirection::Vertical,
        };

        // Place the sprite centred on the *seed* (detector's original
        // text bbox). The seed is always positioned where the source
        // language placed the text — inside the bubble body, never on
        // the tail — so anchoring here keeps translations in the body
        // even when the bubble bbox extends into the tail area.
        //
        // Deliberately no clamp to `expanded_box`: clamping to the
        // segmentation bbox can pull the sprite toward the tail side
        // when the bbox extends past the visible body. Trusting the
        // seed position is both simpler and visually correct.
        let sprite_w = rendered.width() as f32;
        let sprite_h = rendered.height() as f32;
        let seed_cx = seed_box.x + seed_box.width * 0.5;
        let seed_cy = seed_box.y + seed_box.height * 0.5;
        let sx = seed_cx - sprite_w * 0.5;
        let sy = seed_cy - sprite_h * 0.5;
        let centred = Transform {
            x: sx.round(),
            y: sy.round(),
            width: sprite_w,
            height: sprite_h,
            rotation_deg: block.transform.rotation_deg,
        };

        Ok(Some(RenderedBlock {
            node_id: block.node_id,
            sprite: DynamicImage::ImageRgba8(rendered),
            rendered_direction,
            expanded_transform: Some(centred),
        }))
    }

    fn select_font(&self, style: &TextStyle) -> Result<Font> {
        let mut fontbook = self
            .fontbook
            .lock()
            .map_err(|_| anyhow::anyhow!("failed to lock fontbook"))?;
        for candidate in &style.font_families {
            let faces = fontbook.all_families();
            if let Some(psn) = face_post_script_name(&faces, candidate) {
                return fontbook.query(&psn);
            }
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

// ---------------------------------------------------------------------------
// Helpers: font sizing
// ---------------------------------------------------------------------------

fn min_font_size_for_image(image_width: u32, image_height: u32) -> f32 {
    let max_dim = image_width.max(image_height) as f32;
    (max_dim / 90.0).clamp(12.0, 28.0)
}

/// Maximum font size for the given layout box, derived from its dimensions.
/// Caps extreme cases (huge empty bubble + short text → giant glyphs).
fn max_font_size_for_box(layout_box: LayoutBox, min_size: f32) -> f32 {
    const GLOBAL_CAP_PX: f32 = 72.0;
    let by_height = layout_box.height * 0.45;
    let by_width = layout_box.width * 0.9;
    by_height.min(by_width).clamp(min_size + 1.0, GLOBAL_CAP_PX)
}

/// Binary-search the largest integer font size in `[min_size, max_size]`
/// whose shaped layout still fits inside the constraint box. An
/// `explicit_size` override (user-set per-block font size) bypasses the
/// search.
fn fit_font_size<'a>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    constraint_width: f32,
    constraint_height: f32,
    explicit_size: Option<f32>,
    min_size: f32,
    max_size: f32,
) -> Result<LayoutRun<'a>> {
    let run_at = |size: f32| -> Result<LayoutRun<'a>> {
        layout_builder
            .clone()
            .with_font_size(size.max(1.0))
            .with_max_width(constraint_width)
            .with_max_height(constraint_height)
            .run(text)
    };
    if let Some(s) = explicit_size {
        return run_at(s);
    }

    let fits =
        |run: &LayoutRun<'a>| run.width <= constraint_width && run.height <= constraint_height;

    let min_size = min_size.max(1.0).round() as i32;
    let max_size = (max_size.round() as i32).max(min_size);

    let at_max = run_at(max_size as f32)?;
    if fits(&at_max) {
        return Ok(at_max);
    }
    // Binary-search [min, max) for the largest fitting size.
    let mut lo = min_size;
    let mut hi = max_size - 1;
    let mut best = run_at(min_size as f32)?;
    if !fits(&best) {
        return Ok(best);
    }
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let candidate = run_at(mid as f32)?;
        if fits(&candidate) {
            best = candidate;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    Ok(best)
}

// ---------------------------------------------------------------------------
// Helpers: font families, fallbacks
// ---------------------------------------------------------------------------

fn apply_default_font_families(font_families: &mut Vec<String>, text: &str) {
    if font_families.is_empty() {
        *font_families = font_families_for_text(text);
    }
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

// ---------------------------------------------------------------------------
// Helpers: stroke resolution
// ---------------------------------------------------------------------------

fn default_stroke_width(font_size: f32) -> f32 {
    (font_size * 0.10).clamp(1.2, 8.0)
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
    font_prediction: Option<&FontPrediction>,
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
    if let Some(pred) = font_prediction
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

// ---------------------------------------------------------------------------
// Helpers: type conversions
// ---------------------------------------------------------------------------

fn shader_core_to_renderer(e: TextShaderEffect) -> RendererEffect {
    RendererEffect {
        italic: e.italic,
        bold: e.bold,
    }
}

fn core_align_to_renderer(a: koharu_core::TextAlign) -> RendererTextAlign {
    match a {
        koharu_core::TextAlign::Left => RendererTextAlign::Left,
        koharu_core::TextAlign::Center => RendererTextAlign::Center,
        koharu_core::TextAlign::Right => RendererTextAlign::Right,
    }
}

// ---------------------------------------------------------------------------
// Helpers: placement
// ---------------------------------------------------------------------------

fn find_input(blocks: &[RenderBlockInput], id: NodeId) -> &RenderBlockInput {
    blocks
        .iter()
        .find(|b| b.node_id == id)
        .expect("rendered_block must have matching input")
}

fn placement_origin(input: &RenderBlockInput, expanded: &Option<Transform>) -> (f32, f32) {
    if let Some(t) = expanded {
        (t.x.round(), t.y.round())
    } else {
        (input.transform.x, input.transform.y)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_font_families_should_fill_empty_list() {
        let mut font_families = Vec::new();
        apply_default_font_families(&mut font_families, "hello");
        assert!(!font_families.is_empty());
    }

    #[test]
    fn default_stroke_color_uses_black_for_light_text() {
        let stroke = resolve_stroke_style(None, None, None, 16.0, [255, 255, 255, 255])
            .expect("default stroke should be present");
        assert_eq!(stroke.color, [0, 0, 0, 255]);
        assert_eq!(stroke.width_px, 1.6);
    }

    #[test]
    fn predicted_stroke_width_keeps_auto_black_or_white_color() {
        let prediction = FontPrediction {
            stroke_color: [12, 34, 56],
            stroke_width_px: 3.0,
            ..Default::default()
        };
        let stroke =
            resolve_stroke_style(Some(&prediction), None, None, 18.0, [255, 255, 255, 255])
                .expect("predicted stroke should be present");
        assert_eq!(stroke.color, [0, 0, 0, 255]);
        assert_eq!(stroke.width_px, 3.0);
    }

    #[test]
    fn explicit_block_stroke_color_is_preserved_even_if_it_matches_text() {
        let stroke = resolve_stroke_style(
            None,
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
}
