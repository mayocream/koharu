use anyhow::{Context, Result, bail};
use image::RgbaImage;
use skia_safe::{Canvas, Color, Font as SkFont, Paint, PaintStyle, Point, surfaces};

use crate::font::Font;
use crate::layout::{LayoutRun, PositionedGlyph, WritingMode};

/// Options for rendering text.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub color: [u8; 4],
    pub background: Option<[u8; 4]>,
    pub anti_alias: bool,
    pub padding: f32,
    pub font_size: f32,
}

/// Default render options.
impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
            background: None,
            anti_alias: true,
            padding: 0.0,
            font_size: 16.0,
        }
    }
}

/// Skia-based text renderer.
#[derive(Default)]
pub struct SkiaRenderer;

impl SkiaRenderer {
    pub fn new() -> Self {
        Self
    }

    /// Renders the given layout run to an RGBA image.
    pub fn render(
        &self,
        layout: &LayoutRun,
        writing_mode: WritingMode,
        font: &Font,
        opts: &RenderOptions,
    ) -> Result<RgbaImage> {
        let width = (layout.width + opts.padding * 2.0).ceil() as i32;
        let height = (layout.height + opts.padding * 2.0).ceil() as i32;
        if width <= 0 || height <= 0 {
            bail!("invalid surface size {width}x{height}");
        }

        let mut surface = surfaces::raster_n32_premul((width, height))
            .context("failed to create Skia surface")?;
        let canvas = surface.canvas();

        if let Some(bg) = opts.background {
            canvas.clear(Color::from_argb(bg[3], bg[0], bg[1], bg[2]));
        } else {
            canvas.clear(Color::TRANSPARENT);
        }

        let typeface = font.skia()?;
        let sk_font = SkFont::from_typeface(&typeface, opts.font_size);

        let mut paint = Paint::default();
        paint.set_anti_alias(opts.anti_alias);
        paint.set_style(PaintStyle::Fill);
        paint.set_color(Color::from_argb(
            opts.color[3],
            opts.color[2],
            opts.color[1],
            opts.color[0], // ARGB order???
        ));

        for line in &layout.lines {
            let origin = match writing_mode {
                WritingMode::Horizontal => (
                    opts.padding + line.baseline.0,
                    opts.padding + line.baseline.1,
                ),
                WritingMode::VerticalRl => (
                    opts.padding + line.baseline.0,
                    opts.padding + line.baseline.1,
                ),
            };
            draw_glyphs(canvas, &sk_font, &paint, &line.glyphs, origin);
        }

        let info = surface.image_info();
        let row_bytes = info.min_row_bytes();
        let mut pixels = vec![0u8; (height as usize) * row_bytes];
        if !surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0)) {
            bail!("failed to read pixels from surface");
        }
        let img = image::RgbaImage::from_raw(width as u32, height as u32, pixels)
            .context("failed to build RgbaImage")?;
        Ok(img)
    }
}

fn draw_glyphs(
    canvas: &Canvas,
    font: &SkFont,
    paint: &Paint,
    glyphs: &[PositionedGlyph],
    origin: (f32, f32),
) {
    let (origin_x, origin_y) = origin;
    let mut pen_x = 0.0f32;
    let mut pen_y = 0.0f32;

    let mut ids = Vec::with_capacity(glyphs.len());
    let mut positions = Vec::with_capacity(glyphs.len());

    for g in glyphs {
        // Skia glyph IDs are u16; skip any IDs that don't fit (shouldn't happen in normal fonts).
        let Ok(gid) = u16::try_from(g.glyph_id) else {
            pen_x += g.x_advance;
            pen_y -= g.y_advance;
            continue;
        };

        ids.push(gid);
        positions.push(Point::new(pen_x + g.x_offset, pen_y - g.y_offset));

        pen_x += g.x_advance;
        // HarfBuzz/HarfRust positioning uses a Y-up coordinate system; Skia's canvas is Y-down.
        // We already flip `y_offset` above, so we also need to flip the pen advance to keep
        // vertical text flowing top-to-bottom instead of bottom-to-top.
        pen_y -= g.y_advance;
    }

    canvas.draw_glyphs_at(
        &ids,
        positions.as_slice(),
        (origin_x, origin_y),
        font,
        paint,
    );
}
