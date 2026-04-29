//! Koharu text renderer.
//!
//! Owns the font book, symbol fallbacks, and Google Fonts service. Exposes
//! [`Renderer::render_page`], which rasterises each text block's translation
//! into an RGBA sprite and composites them onto the inpainted plane.
//!
//! Pure output: the pipeline engine ([`crate::pipeline::engines::renderer`])
//! takes a `RenderOutput` and translates sprites + final composite into ops.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, RgbaImage, imageops};
use koharu_core::{
    FontFaceInfo, FontPrediction, FontSource, NodeId, TextDirection, TextShaderEffect,
    TextStrokeStyle, TextStyle, Transform,
};

use koharu_renderer::{
    TextAlign as RendererTextAlign, TextShaderEffect as RendererEffect,
    font::{FaceInfo, Font, FontBook},
    layout::{LayoutRun, TextLayout, WritingMode},
    renderer::{RasterOptions, RenderOptions, RenderStrokeOptions, TinySkiaRenderer},
    text::{
        latin::{BubbleIndex, LayoutBox},
        script::{font_families_for_text, writing_mode_for_block},
    },
    types::{RenderBlock, TextDirection as RendererTextDirection},
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
    pub target_language: Option<String>,
    pub raster: RasterOptions,
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
        let layout_boxes = resolve_layout_boxes(blocks, bubble_index.as_ref());
        let bubble_mask = bubble_index.as_ref().map(BubbleIndex::mask);

        let mut rendered_blocks = Vec::with_capacity(blocks.len());
        for (block, layout_box) in blocks.iter().zip(layout_boxes.iter().copied()) {
            match self.render_one(
                block,
                layout_box,
                bubble_mask,
                &opts.shader_effect,
                &opts.shader_stroke,
                opts.document_font.as_deref(),
                opts.target_language.as_deref(),
                opts.raster,
                min_font,
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
        resolved_box: ResolvedLayoutBox,
        bubble_mask: Option<&GrayImage>,
        effect: &TextShaderEffect,
        global_stroke: &Option<TextStrokeStyle>,
        document_font: Option<&str>,
        target_language: Option<&str>,
        raster: RasterOptions,
        min_font_size: f32,
    ) -> Result<Option<RenderedBlock>> {
        let translation = block.translation.trim();
        if translation.is_empty() {
            return Ok(None);
        }

        let layout_source = layout_source_from_input(block, translation);

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
        apply_default_font_families(&mut style.font_families, translation);

        let font = self.select_font(&style)?;
        let block_effect = style.effect.unwrap_or(*effect);
        let color =
            resolve_text_color(block.style.as_ref(), &style, block.font_prediction.as_ref());

        let writing_mode = writing_mode_for_block(&layout_source);
        // Translations default to centre alignment inside a bubble — each
        // line sits centred above/below the others, matching manga
        // typesetting convention. Explicit `style.text_align` wins if set.
        let align = style
            .text_align
            .map(core_align_to_renderer)
            .unwrap_or(RendererTextAlign::Center);
        let seed_box = resolved_box.seed_box;
        let layout_box = resolved_box.layout_box;

        let mut layout_builder = TextLayout::new(&font, None)
            .with_fallback_fonts(&self.symbol_fallbacks)
            .with_writing_mode(writing_mode)
            .with_alignment(align);
        if let Some(target_language) = target_language {
            layout_builder = layout_builder.with_hyphenation_language_tag(target_language);
        }
        let max_font = max_font_size_for_box(layout_box, min_font_size);
        let mut render_candidate = |layout: &LayoutRun<'_>| -> Result<RenderedTextCandidate> {
            let resolved_stroke = resolve_stroke_style(
                block.font_prediction.as_ref(),
                style.stroke.as_ref(),
                global_stroke.as_ref(),
                layout.font_size,
                color,
            );

            let rendered = self.renderer.render(
                layout,
                writing_mode,
                &RenderOptions {
                    font_size: layout.font_size,
                    color,
                    effect: shader_core_to_renderer(block_effect),
                    stroke: resolved_stroke,
                    raster,
                    ..Default::default()
                },
            )?;
            let transform = centred_sprite_transform(
                seed_box,
                rendered.width(),
                rendered.height(),
                block.transform.rotation_deg,
            );
            Ok(RenderedTextCandidate {
                image: rendered,
                transform,
            })
        };

        if let Some((mask, bubble_id)) = bubble_mask.zip(resolved_box.bubble_id) {
            let candidate = fit_rendered_with_mask_collision(
                &layout_builder,
                translation,
                writing_mode,
                layout_box,
                style.font_size,
                min_font_size,
                max_font,
                mask,
                bubble_id,
                &mut render_candidate,
            )?;
            return Ok(Some(RenderedBlock {
                node_id: block.node_id,
                sprite: DynamicImage::ImageRgba8(candidate.image),
                rendered_direction: rendered_direction_for_writing_mode(writing_mode),
                expanded_transform: Some(candidate.transform),
            }));
        }

        let layout = fit_font_size(
            &layout_builder,
            translation,
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
                .run(translation)?
        } else {
            layout
        };

        let candidate = render_candidate(&layout)?;

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
        Ok(Some(RenderedBlock {
            node_id: block.node_id,
            sprite: DynamicImage::ImageRgba8(candidate.image),
            rendered_direction: rendered_direction_for_writing_mode(writing_mode),
            expanded_transform: Some(candidate.transform),
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

const MASK_COLLISION_ALPHA_THRESHOLD: u8 = 8;
const COLLISION_SQUEEZE_FACTOR: f32 = 0.90;
const COLLISION_SQUEEZE_ATTEMPTS: usize = 3;
const FIT_EPSILON: f32 = 0.5;

struct RenderedTextCandidate {
    image: RgbaImage,
    transform: Transform,
}

struct CollisionFitAttempt {
    valid: Option<RenderedTextCandidate>,
    fallback: Option<RenderedTextCandidate>,
}

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

#[allow(clippy::too_many_arguments)]
fn fit_rendered_with_mask_collision<'a, F>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    writing_mode: WritingMode,
    layout_box: LayoutBox,
    explicit_size: Option<f32>,
    min_size: f32,
    max_size: f32,
    mask: &GrayImage,
    bubble_id: u8,
    render_candidate: &mut F,
) -> Result<RenderedTextCandidate>
where
    F: FnMut(&LayoutRun<'a>) -> Result<RenderedTextCandidate>,
{
    let upper = explicit_size.unwrap_or(max_size).max(1.0).round() as i32;
    let lower = min_size.max(1.0).round() as i32;
    let min_size = if explicit_size.is_some() {
        lower.min(upper)
    } else {
        lower
    };
    let max_size = upper.max(min_size);

    let min_attempt = try_mask_collision_size(
        layout_builder,
        text,
        writing_mode,
        layout_box,
        min_size as f32,
        mask,
        bubble_id,
        render_candidate,
    )?;
    let Some(mut best) = min_attempt.valid else {
        if let Some(fallback) = min_attempt.fallback {
            return Ok(fallback);
        }
        return render_collision_fallback(
            layout_builder,
            text,
            writing_mode,
            layout_box,
            min_size as f32,
            render_candidate,
        );
    };

    let mut lo = min_size + 1;
    let mut hi = max_size;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let attempt = try_mask_collision_size(
            layout_builder,
            text,
            writing_mode,
            layout_box,
            mid as f32,
            mask,
            bubble_id,
            render_candidate,
        )?;
        if let Some(candidate) = attempt.valid {
            best = candidate;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }

    Ok(best)
}

#[allow(clippy::too_many_arguments)]
fn try_mask_collision_size<'a, F>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    writing_mode: WritingMode,
    layout_box: LayoutBox,
    font_size: f32,
    mask: &GrayImage,
    bubble_id: u8,
    render_candidate: &mut F,
) -> Result<CollisionFitAttempt>
where
    F: FnMut(&LayoutRun<'a>) -> Result<RenderedTextCandidate>,
{
    let mut fallback = None;
    for main_extent in collision_squeeze_extents(layout_box, writing_mode) {
        let layout = run_collision_layout_at(
            layout_builder,
            text,
            writing_mode,
            layout_box,
            font_size,
            main_extent,
        )?;
        if !layout_fits_collision_attempt(&layout, writing_mode, layout_box, main_extent) {
            continue;
        }
        let candidate = render_candidate(&layout)?;
        if sprite_collides_with_bubble_mask(&candidate.image, &candidate.transform, mask, bubble_id)
        {
            fallback = Some(candidate);
            continue;
        }
        return Ok(CollisionFitAttempt {
            valid: Some(candidate),
            fallback,
        });
    }

    Ok(CollisionFitAttempt {
        valid: None,
        fallback,
    })
}

fn render_collision_fallback<'a, F>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    writing_mode: WritingMode,
    layout_box: LayoutBox,
    font_size: f32,
    render_candidate: &mut F,
) -> Result<RenderedTextCandidate>
where
    F: FnMut(&LayoutRun<'a>) -> Result<RenderedTextCandidate>,
{
    let layout = run_collision_layout_at(
        layout_builder,
        text,
        writing_mode,
        layout_box,
        font_size,
        primary_collision_extent(layout_box, writing_mode),
    )?;
    render_candidate(&layout)
}

fn run_collision_layout_at<'a>(
    layout_builder: &TextLayout<'a>,
    text: &str,
    writing_mode: WritingMode,
    layout_box: LayoutBox,
    font_size: f32,
    main_extent: f32,
) -> Result<LayoutRun<'a>> {
    let (max_width, max_height) = match writing_mode {
        WritingMode::Horizontal => (main_extent, layout_box.height),
        WritingMode::VerticalRl => (layout_box.width, main_extent),
    };
    layout_builder
        .clone()
        .with_font_size(font_size.max(1.0))
        .with_max_width(max_width.max(1.0))
        .with_max_height(max_height.max(1.0))
        .run(text)
}

fn layout_fits_collision_attempt(
    layout: &LayoutRun<'_>,
    writing_mode: WritingMode,
    layout_box: LayoutBox,
    main_extent: f32,
) -> bool {
    let fits_box = layout.width <= layout_box.width + FIT_EPSILON
        && layout.height <= layout_box.height + FIT_EPSILON;
    let fits_main = match writing_mode {
        WritingMode::Horizontal => layout.width <= main_extent + FIT_EPSILON,
        WritingMode::VerticalRl => layout.height <= main_extent + FIT_EPSILON,
    };
    fits_box && fits_main
}

fn collision_squeeze_extents(layout_box: LayoutBox, writing_mode: WritingMode) -> Vec<f32> {
    let mut extents = Vec::with_capacity(COLLISION_SQUEEZE_ATTEMPTS);
    let mut extent = primary_collision_extent(layout_box, writing_mode);
    for _ in 0..COLLISION_SQUEEZE_ATTEMPTS {
        extents.push(extent.max(1.0));
        extent *= COLLISION_SQUEEZE_FACTOR;
    }
    extents
}

fn primary_collision_extent(layout_box: LayoutBox, writing_mode: WritingMode) -> f32 {
    match writing_mode {
        WritingMode::Horizontal => layout_box.width,
        WritingMode::VerticalRl => layout_box.height,
    }
}

fn sprite_collides_with_bubble_mask(
    sprite: &RgbaImage,
    transform: &Transform,
    mask: &GrayImage,
    bubble_id: u8,
) -> bool {
    let origin_x = transform.x.round() as i32;
    let origin_y = transform.y.round() as i32;
    let mask_w = mask.width() as i32;
    let mask_h = mask.height() as i32;

    for (x, y, pixel) in sprite.enumerate_pixels() {
        if pixel.0[3] <= MASK_COLLISION_ALPHA_THRESHOLD {
            continue;
        }
        let mask_x = origin_x + x as i32;
        let mask_y = origin_y + y as i32;
        if mask_x < 0 || mask_y < 0 || mask_x >= mask_w || mask_y >= mask_h {
            return true;
        }
        if mask.get_pixel(mask_x as u32, mask_y as u32).0[0] != bubble_id {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ResolvedLayoutBox {
    seed_box: LayoutBox,
    layout_box: LayoutBox,
    bubble_id: Option<u8>,
}

fn resolve_layout_boxes(
    blocks: &[RenderBlockInput],
    bubble_index: Option<&BubbleIndex>,
) -> Vec<ResolvedLayoutBox> {
    let Some(bubble_index) = bubble_index else {
        return blocks
            .iter()
            .map(|block| {
                let seed_box = seed_layout_box(block);
                ResolvedLayoutBox {
                    seed_box,
                    layout_box: seed_box,
                    bubble_id: None,
                }
            })
            .collect();
    };

    let mut counts: HashMap<u8, usize> = HashMap::new();
    let mut matches = Vec::with_capacity(blocks.len());

    for block in blocks {
        let seed_box = seed_layout_box(block);
        let translation = block.translation.trim();
        let bubble_match = if block.lock_layout_box || translation.is_empty() {
            None
        } else {
            let layout_source = layout_source_from_input(block, translation);
            let writing_mode = writing_mode_for_block(&layout_source);
            bubble_index.lookup_match(seed_box, writing_mode)
        };
        if let Some(matched) = bubble_match {
            *counts.entry(matched.id).or_insert(0) += 1;
        }
        matches.push((seed_box, bubble_match));
    }

    matches
        .into_iter()
        .map(|(seed_box, bubble_match)| match bubble_match {
            // Connected bubbles can contain multiple independently detected
            // text blocks. Expanding all of them to the same bubble bbox makes
            // their layouts collide, so shared bubbles fall back to each
            // block's original detector box.
            Some(matched) if counts.get(&matched.id).copied().unwrap_or(0) == 1 => {
                ResolvedLayoutBox {
                    seed_box,
                    layout_box: matched.layout_box,
                    bubble_id: Some(matched.id),
                }
            }
            Some(matched) => ResolvedLayoutBox {
                seed_box,
                layout_box: seed_box,
                bubble_id: Some(matched.id),
            },
            None => ResolvedLayoutBox {
                seed_box,
                layout_box: seed_box,
                bubble_id: None,
            },
        })
        .collect()
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

fn layout_source_from_input(block: &RenderBlockInput, translation: &str) -> RenderBlock {
    RenderBlock {
        x: block.transform.x,
        y: block.transform.y,
        width: block.transform.width.max(1.0),
        height: block.transform.height.max(1.0),
        text: translation.to_string(),
        source_direction: block.source_direction.map(core_direction_to_renderer),
    }
}

fn seed_layout_box(block: &RenderBlockInput) -> LayoutBox {
    LayoutBox {
        x: block.transform.x,
        y: block.transform.y,
        width: block.transform.width.max(1.0),
        height: block.transform.height.max(1.0),
    }
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

fn resolve_text_color(
    explicit_style: Option<&TextStyle>,
    derived_style: &TextStyle,
    font_prediction: Option<&FontPrediction>,
) -> [u8; 4] {
    if explicit_style.is_some() {
        return derived_style.color;
    }
    if let Some(pred) = font_prediction {
        return [
            pred.text_color[0],
            pred.text_color[1],
            pred.text_color[2],
            255,
        ];
    }
    [0, 0, 0, 255]
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

fn core_direction_to_renderer(d: TextDirection) -> RendererTextDirection {
    match d {
        TextDirection::Horizontal => RendererTextDirection::Horizontal,
        TextDirection::Vertical => RendererTextDirection::Vertical,
    }
}

fn rendered_direction_for_writing_mode(writing_mode: WritingMode) -> TextDirection {
    match writing_mode {
        WritingMode::Horizontal => TextDirection::Horizontal,
        WritingMode::VerticalRl => TextDirection::Vertical,
    }
}

// ---------------------------------------------------------------------------
// Helpers: placement
// ---------------------------------------------------------------------------

fn centred_sprite_transform(
    seed_box: LayoutBox,
    sprite_width: u32,
    sprite_height: u32,
    rotation_deg: f32,
) -> Transform {
    // Place the sprite centred on the seed (detector's original text bbox).
    // The seed is inside the bubble body, so this avoids tail-side drift when
    // the matched bubble bbox extends into a balloon tail.
    let sprite_w = sprite_width as f32;
    let sprite_h = sprite_height as f32;
    let seed_cx = seed_box.x + seed_box.width * 0.5;
    let seed_cy = seed_box.y + seed_box.height * 0.5;
    Transform {
        x: (seed_cx - sprite_w * 0.5).round(),
        y: (seed_cy - sprite_h * 0.5).round(),
        width: sprite_w,
        height: sprite_h,
        rotation_deg,
    }
}

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
    use image::{GrayImage, Luma, Rgba, RgbaImage};
    use koharu_core::NodeId;

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

    #[test]
    fn predicted_text_color_wins_without_explicit_style() {
        let derived = TextStyle {
            font_families: Vec::new(),
            font_size: None,
            color: [0, 0, 0, 255],
            effect: None,
            stroke: None,
            text_align: None,
        };
        let prediction = FontPrediction {
            text_color: [12, 34, 56],
            ..Default::default()
        };
        assert_eq!(
            resolve_text_color(None, &derived, Some(&prediction)),
            [12, 34, 56, 255]
        );
    }

    #[test]
    fn explicit_text_color_wins_over_prediction() {
        let explicit = TextStyle {
            font_families: Vec::new(),
            font_size: None,
            color: [200, 100, 50, 255],
            effect: None,
            stroke: None,
            text_align: None,
        };
        let prediction = FontPrediction {
            text_color: [12, 34, 56],
            ..Default::default()
        };
        assert_eq!(
            resolve_text_color(Some(&explicit), &explicit, Some(&prediction)),
            [200, 100, 50, 255]
        );
    }

    #[test]
    fn shared_bubble_falls_back_to_seed_boxes() {
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 10, 10, 190, 190, 1);
        let index = BubbleIndex::new(mask);
        let blocks = vec![
            block(30.0, 30.0, 40.0, 80.0, "hello"),
            block(120.0, 30.0, 40.0, 80.0, "world"),
        ];

        let layout_boxes = resolve_layout_boxes(&blocks, Some(&index));

        assert_eq!(layout_boxes[0].layout_box, seed_layout_box(&blocks[0]));
        assert_eq!(layout_boxes[0].bubble_id, Some(1));
        assert_eq!(layout_boxes[1].layout_box, seed_layout_box(&blocks[1]));
        assert_eq!(layout_boxes[1].bubble_id, Some(1));
    }

    #[test]
    fn single_block_can_still_expand_into_its_bubble() {
        let mut mask = GrayImage::from_pixel(200, 200, Luma([0u8]));
        paint_rect(&mut mask, 20, 20, 180, 180, 1);
        let index = BubbleIndex::new(mask);
        let blocks = vec![block(70.0, 70.0, 20.0, 30.0, "hello")];

        let layout_boxes = resolve_layout_boxes(&blocks, Some(&index));

        assert!(layout_boxes[0].layout_box.width > blocks[0].transform.width);
        assert!(layout_boxes[0].layout_box.height > blocks[0].transform.height);
        assert_eq!(layout_boxes[0].bubble_id, Some(1));
    }

    #[test]
    fn mask_collision_detects_alpha_outside_matched_bubble() {
        let mut mask = GrayImage::from_pixel(10, 10, Luma([0u8]));
        paint_rect(&mut mask, 2, 2, 8, 8, 1);
        let sprite = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));

        let inside = Transform {
            x: 3.0,
            y: 3.0,
            width: 4.0,
            height: 4.0,
            rotation_deg: 0.0,
        };
        assert!(!sprite_collides_with_bubble_mask(
            &sprite, &inside, &mask, 1
        ));

        let outside = Transform {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
            rotation_deg: 0.0,
        };
        assert!(sprite_collides_with_bubble_mask(
            &sprite, &outside, &mask, 1
        ));
    }

    #[test]
    fn mask_collision_ignores_transparent_sprite_pixels() {
        let mask = GrayImage::from_pixel(4, 4, Luma([0u8]));
        let sprite = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        let transform = Transform {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
            rotation_deg: 0.0,
        };

        assert!(!sprite_collides_with_bubble_mask(
            &sprite, &transform, &mask, 1
        ));
    }

    #[test]
    fn collision_squeeze_extents_retry_the_primary_axis() {
        let layout_box = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };

        assert_eq!(
            collision_squeeze_extents(layout_box, WritingMode::Horizontal),
            vec![100.0, 90.0, 81.0]
        );
        assert_eq!(
            collision_squeeze_extents(layout_box, WritingMode::VerticalRl),
            vec![50.0, 45.0, 40.5]
        );
    }

    fn block(x: f32, y: f32, width: f32, height: f32, translation: &str) -> RenderBlockInput {
        RenderBlockInput {
            node_id: NodeId::new(),
            transform: Transform {
                x,
                y,
                width,
                height,
                rotation_deg: 0.0,
            },
            translation: translation.to_string(),
            style: None,
            font_prediction: None,
            source_direction: None,
            rendered_direction: None,
            lock_layout_box: false,
        }
    }

    fn paint_rect(img: &mut GrayImage, x0: u32, y0: u32, x1: u32, y1: u32, value: u8) {
        for y in y0..y1 {
            for x in x0..x1 {
                img.put_pixel(x, y, Luma([value]));
            }
        }
    }
}
