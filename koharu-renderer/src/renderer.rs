use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use skrifa::{
    GlyphId, MetadataProvider, OutlineGlyph,
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
};
use tiny_skia::{
    Color, FillRule, FilterQuality, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap,
    PixmapPaint, Stroke, Transform,
};

use crate::font::{Font, font_key};
use crate::layout::{LayoutLine, LayoutRun, WritingMode};

pub use crate::types::TextShaderEffect;

#[derive(Debug, Clone, Copy)]
pub struct RenderStrokeOptions {
    pub color: [u8; 4],
    pub width_px: f32,
}

/// Options for rendering text.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub color: [u8; 4],
    pub background: Option<[u8; 4]>,
    pub anti_alias: bool,
    pub padding: f32,
    pub font_size: f32,
    pub effect: TextShaderEffect,
    pub stroke: Option<RenderStrokeOptions>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
            background: None,
            anti_alias: true,
            padding: 0.0,
            font_size: 16.0,
            effect: TextShaderEffect::default(),
            stroke: None,
        }
    }
}

pub struct TinySkiaRenderer;

impl TinySkiaRenderer {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    pub fn render(
        &self,
        layout: &LayoutRun<'_>,
        writing_mode: WritingMode,
        opts: &RenderOptions,
    ) -> Result<RgbaImage> {
        let width = (layout.width + opts.padding * 2.0).ceil() as u32;
        let height = (layout.height + opts.padding * 2.0).ceil() as u32;
        if width == 0 || height == 0 {
            bail!("invalid surface size {width}x{height}");
        }

        let mut surface =
            Pixmap::new(width, height).context("failed to allocate render surface")?;
        if let Some(bg) = opts.background {
            surface.fill(color_from_rgba(bg));
        }

        let mut cache: HashMap<FontGlyphId, GlyphRenderSource> = HashMap::new();
        let has_stroke = opts
            .stroke
            .is_some_and(|stroke| stroke.width_px > 0.0 && stroke.color[3] > 0);
        if has_stroke {
            render_pass(
                &mut surface,
                &mut cache,
                layout,
                writing_mode,
                opts,
                RenderPass::Stroke,
            )?;
        }
        render_pass(
            &mut surface,
            &mut cache,
            layout,
            writing_mode,
            opts,
            RenderPass::Fill,
        )?;

        let mut pixels = surface.data().to_vec();
        unpremultiply_rgba(&mut pixels);
        let img =
            RgbaImage::from_raw(width, height, pixels).context("failed to build RgbaImage")?;
        Ok(img)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FontGlyphId {
    font: usize,
    glyph: u16,
}

#[derive(Clone, Copy, Debug)]
struct GlyphMetrics {
    width: u32,
    height: u32,
    xmin: i32,
    ymin: i32,
}

enum GlyphRenderSource {
    Outline(OutlineGlyphData),
    Bitmap(BitmapGlyphData),
}

struct OutlineGlyphData {
    path: Path,
    bounds: tiny_skia::Rect,
}

struct BitmapGlyphData {
    metrics: GlyphMetrics,
    fill_alpha: Vec<u8>,
}

#[derive(Clone, Copy)]
enum RenderPass {
    Stroke,
    Fill,
}

fn render_pass(
    surface: &mut Pixmap,
    cache: &mut HashMap<FontGlyphId, GlyphRenderSource>,
    layout: &LayoutRun<'_>,
    writing_mode: WritingMode,
    opts: &RenderOptions,
    pass: RenderPass,
) -> Result<()> {
    for line in &layout.lines {
        let origin = match writing_mode {
            WritingMode::Horizontal | WritingMode::VerticalRl => (
                opts.padding + line.baseline.0,
                opts.padding + line.baseline.1,
            ),
        };
        render_line(surface, cache, line, origin, opts, pass)?;
    }

    Ok(())
}

fn render_line(
    surface: &mut Pixmap,
    cache: &mut HashMap<FontGlyphId, GlyphRenderSource>,
    line: &LayoutLine<'_>,
    origin: (f32, f32),
    opts: &RenderOptions,
    pass: RenderPass,
) -> Result<()> {
    let (origin_x, origin_y) = origin;
    let mut pen_x = 0.0f32;
    let mut pen_y = 0.0f32;

    for glyph in &line.glyphs {
        let Ok(gid) = u16::try_from(glyph.glyph_id) else {
            pen_x += glyph.x_advance;
            pen_y -= glyph.y_advance;
            continue;
        };

        let key = FontGlyphId {
            font: font_key(glyph.font),
            glyph: gid,
        };
        if let std::collections::hash_map::Entry::Vacant(e) = cache.entry(key) {
            let source = load_glyph_source(glyph.font, gid, opts.font_size, opts.anti_alias)?;
            e.insert(source);
        }

        let baseline_x = origin_x + pen_x + glyph.x_offset;
        let baseline_y = origin_y + pen_y - glyph.y_offset;

        if let Some(source) = cache.get(&key) {
            match source {
                GlyphRenderSource::Outline(data) => {
                    draw_outline_glyph(surface, data, baseline_x, baseline_y, opts, pass);
                }
                GlyphRenderSource::Bitmap(data) => {
                    draw_bitmap_glyph(surface, data, baseline_x, baseline_y, opts, pass)?;
                }
            }
        }

        pen_x += glyph.x_advance;
        pen_y -= glyph.y_advance;
    }

    Ok(())
}

fn load_glyph_source(
    font: &Font,
    glyph_id: u16,
    font_size: f32,
    anti_alias: bool,
) -> Result<GlyphRenderSource> {
    let font_ref = font.skrifa()?;
    if let Some(outline) = font_ref.outline_glyphs().get(GlyphId::new(glyph_id as u32))
        && let Some(path) = outline_to_path(&outline, font_size)
    {
        let bounds = path.bounds();
        if bounds.width() > 0.0 && bounds.height() > 0.0 {
            return Ok(GlyphRenderSource::Outline(OutlineGlyphData {
                path,
                bounds,
            }));
        }
    }

    let fontdue = font.fontdue()?;
    let (metrics, mut bitmap) = fontdue.rasterize_indexed(glyph_id, font_size);
    if !anti_alias {
        for px in &mut bitmap {
            *px = if *px >= 128 { 255 } else { 0 };
        }
    }

    Ok(GlyphRenderSource::Bitmap(BitmapGlyphData {
        metrics: GlyphMetrics {
            width: metrics.width as u32,
            height: metrics.height as u32,
            xmin: metrics.xmin,
            ymin: metrics.ymin,
        },
        fill_alpha: bitmap,
    }))
}

fn draw_outline_glyph(
    surface: &mut Pixmap,
    glyph: &OutlineGlyphData,
    baseline_x: f32,
    baseline_y: f32,
    opts: &RenderOptions,
    pass: RenderPass,
) {
    let transform = glyph_transform(glyph.bounds, baseline_x, baseline_y, opts.effect.italic);

    match pass {
        RenderPass::Stroke => {
            if let Some(stroke) = opts
                .stroke
                .filter(|stroke| stroke.width_px > 0.0 && stroke.color[3] > 0)
            {
                let stroke_paint = paint_from_rgba(stroke.color, opts.anti_alias);
                let stroke_style = Stroke {
                    width: stroke.width_px.max(0.0) * 2.0,
                    line_join: LineJoin::Round,
                    line_cap: LineCap::Round,
                    ..Default::default()
                };
                surface.stroke_path(&glyph.path, &stroke_paint, &stroke_style, transform, None);
            }
        }
        RenderPass::Fill => {
            if opts.effect.bold {
                let bold_paint = paint_from_rgba(opts.color, opts.anti_alias);
                let bold_style = Stroke {
                    width: 2.0,
                    line_join: LineJoin::Round,
                    line_cap: LineCap::Round,
                    ..Default::default()
                };
                surface.stroke_path(&glyph.path, &bold_paint, &bold_style, transform, None);
            }

            let fill_paint = paint_from_rgba(opts.color, opts.anti_alias);
            surface.fill_path(&glyph.path, &fill_paint, FillRule::Winding, transform, None);
        }
    }
}

fn draw_bitmap_glyph(
    surface: &mut Pixmap,
    glyph: &BitmapGlyphData,
    baseline_x: f32,
    baseline_y: f32,
    opts: &RenderOptions,
    pass: RenderPass,
) -> Result<()> {
    if glyph.metrics.width == 0 || glyph.metrics.height == 0 || glyph.fill_alpha.is_empty() {
        return Ok(());
    }

    let width = glyph.metrics.width as usize;
    let height = glyph.metrics.height as usize;
    let mut fill_alpha = glyph.fill_alpha.clone();
    if opts.effect.bold {
        fill_alpha = dilate_alpha(&fill_alpha, width, height, 1);
    }

    let x = baseline_x + glyph.metrics.xmin as f32;
    let y = baseline_y - glyph.metrics.ymin as f32 - glyph.metrics.height as f32;
    let transform = bitmap_transform(
        glyph.metrics.width as f32,
        glyph.metrics.height as f32,
        x,
        y,
        opts.effect.italic,
    );
    let paint = pixmap_paint(opts.effect.italic, opts.anti_alias);

    match pass {
        RenderPass::Stroke => {
            if let Some(stroke) = opts
                .stroke
                .filter(|stroke| stroke.width_px > 0.0 && stroke.color[3] > 0)
            {
                let radius = stroke.width_px.ceil().max(1.0) as usize;
                let outer = dilate_alpha(&fill_alpha, width, height, radius);
                let stroke_alpha = outer
                    .into_iter()
                    .zip(&fill_alpha)
                    .map(|(outer_alpha, fill)| outer_alpha.saturating_sub(*fill))
                    .collect::<Vec<_>>();
                if let Some(stroke_pixmap) = alpha_pixmap(
                    glyph.metrics.width,
                    glyph.metrics.height,
                    &stroke_alpha,
                    stroke.color,
                ) {
                    surface.draw_pixmap(0, 0, stroke_pixmap.as_ref(), &paint, transform, None);
                }
            }
        }
        RenderPass::Fill => {
            if let Some(fill_pixmap) = alpha_pixmap(
                glyph.metrics.width,
                glyph.metrics.height,
                &fill_alpha,
                opts.color,
            ) {
                surface.draw_pixmap(0, 0, fill_pixmap.as_ref(), &paint, transform, None);
            }
        }
    }

    Ok(())
}

fn glyph_transform(
    bounds: tiny_skia::Rect,
    baseline_x: f32,
    baseline_y: f32,
    italic: bool,
) -> Transform {
    if !italic {
        return Transform::from_translate(baseline_x, baseline_y);
    }

    let glyph_w = bounds.width().max(1.0);
    let glyph_h = bounds.height().max(1.0);
    let slant = (glyph_w.min(glyph_h) * 0.22).max(1.0);
    let kx = -slant / glyph_h;
    Transform::from_row(
        1.0,
        0.0,
        kx,
        1.0,
        baseline_x - kx * bounds.bottom(),
        baseline_y,
    )
}

fn bitmap_transform(width: f32, height: f32, x: f32, y: f32, italic: bool) -> Transform {
    if !italic {
        return Transform::from_translate(x, y);
    }

    let glyph_w = width.max(1.0);
    let glyph_h = height.max(1.0);
    let slant = (glyph_w.min(glyph_h) * 0.22).max(1.0);
    let kx = -slant / glyph_h;
    Transform::from_row(1.0, 0.0, kx, 1.0, x - kx * glyph_h, y)
}

fn pixmap_paint(italic: bool, anti_alias: bool) -> PixmapPaint {
    PixmapPaint {
        quality: if italic && anti_alias {
            FilterQuality::Bilinear
        } else {
            FilterQuality::Nearest
        },
        ..Default::default()
    }
}

fn paint_from_rgba(color: [u8; 4], anti_alias: bool) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias,
        ..Default::default()
    };
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint
}

fn color_from_rgba(color: [u8; 4]) -> Color {
    Color::from_rgba8(color[0], color[1], color[2], color[3])
}

fn outline_to_path(outline: &OutlineGlyph<'_>, font_size: f32) -> Option<Path> {
    let mut pen = TinySkiaPathPen::new();
    let settings = DrawSettings::unhinted(Size::new(font_size), LocationRef::default());
    outline.draw(settings, &mut pen).ok()?;
    pen.finish()
}

fn alpha_pixmap(width: u32, height: u32, alpha: &[u8], color: [u8; 4]) -> Option<Pixmap> {
    if color[3] == 0 || width == 0 || height == 0 || alpha.is_empty() {
        return None;
    }

    let mut pixmap = Pixmap::new(width, height)?;
    let data = pixmap.data_mut();
    for (index, &mask_alpha) in alpha.iter().enumerate() {
        let out_alpha = ((mask_alpha as u32 * color[3] as u32) + 127) / 255;
        let offset = index * 4;
        data[offset] = (((color[0] as u32 * out_alpha) + 127) / 255) as u8;
        data[offset + 1] = (((color[1] as u32 * out_alpha) + 127) / 255) as u8;
        data[offset + 2] = (((color[2] as u32 * out_alpha) + 127) / 255) as u8;
        data[offset + 3] = out_alpha as u8;
    }
    Some(pixmap)
}

fn dilate_alpha(alpha: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    if radius == 0 || alpha.is_empty() {
        return alpha.to_vec();
    }

    let mut out = vec![0u8; alpha.len()];
    for y in 0..height {
        let y0 = y.saturating_sub(radius);
        let y1 = (y + radius).min(height.saturating_sub(1));
        for x in 0..width {
            let x0 = x.saturating_sub(radius);
            let x1 = (x + radius).min(width.saturating_sub(1));
            let mut max_alpha = 0u8;
            for yy in y0..=y1 {
                let row = yy * width;
                for xx in x0..=x1 {
                    max_alpha = max_alpha.max(alpha[row + xx]);
                }
            }
            out[y * width + x] = max_alpha;
        }
    }
    out
}

fn unpremultiply_rgba(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 || a == 255 {
            continue;
        }
        let alpha = a as u32;
        px[0] = ((px[0] as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
        px[1] = ((px[1] as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
        px[2] = ((px[2] as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
    }
}

struct TinySkiaPathPen {
    builder: PathBuilder,
}

impl TinySkiaPathPen {
    fn new() -> Self {
        Self {
            builder: PathBuilder::new(),
        }
    }

    fn finish(self) -> Option<Path> {
        self.builder.finish()
    }
}

impl OutlinePen for TinySkiaPathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(x, -y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(x, -y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.builder.quad_to(cx0, -cy0, x, -y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.builder.cubic_to(cx0, -cy0, cx1, -cy1, x, -y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}
