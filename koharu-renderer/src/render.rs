//! High-performance text rasterization with parallel glyph processing.
//!
//! This module handles the final stage of text rendering: converting positioned
//! glyphs into raster images. It uses parallel processing to efficiently rasterize
//! large numbers of glyphs and composite them into the final output image.

use anyhow::Result;
use image::{Rgba, RgbaImage};
use rayon::prelude::*;
use swash::scale::image::{Content, Image as GlyphImage};
use swash::scale::{Render, ScaleContext, Scaler, Source, StrikeWith};
use swash::shape::cluster::Glyph;
use swash::zeno::Vector;

use crate::layout::LayoutResult;
use crate::types::{Color, Point, TextStyle};

#[derive(Debug)]
pub struct RenderRequest<'a> {
    pub style: TextStyle<'a>,
    pub layout: &'a LayoutResult,
    pub background: Color,
}

#[derive(Debug)]
pub struct RenderedText {
    pub image: RgbaImage,
    pub origin: Point,
}

struct RasterGlyph {
    image: GlyphImage,
    left: f32,
    top: f32,
}

#[derive(Clone, Copy)]
struct GlyphBounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl GlyphBounds {
    fn from_glyph(glyph: &RasterGlyph) -> Self {
        let right = glyph.left + glyph.image.placement.width as f32;
        let bottom = glyph.top + glyph.image.placement.height as f32;
        Self {
            min_x: glyph.left,
            min_y: glyph.top,
            max_x: right,
            max_y: bottom,
        }
    }

    fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    fn height(&self) -> f32 {
        self.max_y - self.min_y
    }
}

pub struct TextRenderer {
    scale_context: ScaleContext,
    sources: [Source; 3],
}

impl TextRenderer {
    pub fn new() -> Self {
        Self {
            scale_context: ScaleContext::new(),
            sources: [
                Source::ColorOutline(0),
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::Outline,
            ],
        }
    }

    pub fn render(&mut self, request: &RenderRequest<'_>) -> Result<RenderedText> {
        if request.layout.is_empty() {
            return Ok(Self::empty_render(request.background));
        }

        let font_ref = request.style.font.font_ref()?;

        let mut scaler = self
            .scale_context
            .builder(font_ref)
            .size(request.style.font_size.max(0.0))
            .build();

        let glyphs = Self::flatten_glyphs(request.layout);
        let rasterized =
            Self::rasterize_glyphs(&self.sources, &mut scaler, glyphs, request.style.color)?;

        if rasterized.is_empty() {
            return Ok(Self::empty_render(request.background));
        }

        let bounds = self.calculate_bounds(&rasterized);
        let image =
            self.composite_surface(bounds, &rasterized, request.style.color, request.background);

        Ok(RenderedText {
            image,
            origin: (bounds.min_x, bounds.min_y),
        })
    }

    fn flatten_glyphs(layout: &LayoutResult) -> Vec<Glyph> {
        layout
            .par_iter()
            .flat_map(|line| line.glyphs.par_iter())
            .cloned()
            .collect()
    }

    fn rasterize_glyphs(
        sources: &[Source; 3],
        scaler: &mut Scaler,
        glyphs: Vec<Glyph>,
        color: Color,
    ) -> Result<Vec<RasterGlyph>> {
        let mut rasterized = Vec::with_capacity(glyphs.len());
        for glyph in glyphs {
            if let Some(raster_glyph) = rasterize_glyph(sources, scaler, &glyph, color)? {
                rasterized.push(raster_glyph);
            }
        }
        Ok(rasterized)
    }

    fn composite_surface(
        &self,
        bounds: GlyphBounds,
        rasterized: &[RasterGlyph],
        color: Color,
        background: Color,
    ) -> RgbaImage {
        let width = bounds.width().ceil().max(1.0) as u32;
        let height = bounds.height().ceil().max(1.0) as u32;
        let mut surface = RgbaImage::from_pixel(width, height, Rgba(background));
        let blit_positions: Vec<_> = rasterized
            .par_iter()
            .map(|glyph| {
                let dest_x = (glyph.left - bounds.min_x).floor() as i32;
                let dest_y = (glyph.top - bounds.min_y).floor() as i32;
                (dest_x, dest_y, glyph)
            })
            .collect();
        for (dest_x, dest_y, glyph) in blit_positions {
            blit_glyph(&mut surface, dest_x, dest_y, glyph, color);
        }
        surface
    }

    fn empty_render(background: Color) -> RenderedText {
        RenderedText {
            image: RgbaImage::from_pixel(1, 1, Rgba(background)),
            origin: (0.0, 0.0),
        }
    }

    fn calculate_bounds(&self, rasterized: &[RasterGlyph]) -> GlyphBounds {
        rasterized
            .par_iter()
            .map(|glyph| GlyphBounds::from_glyph(glyph))
            .reduce(
                || GlyphBounds::from_glyph(&rasterized[0]),
                |a, b| GlyphBounds {
                    min_x: a.min_x.min(b.min_x),
                    min_y: a.min_y.min(b.min_y),
                    max_x: a.max_x.max(b.max_x),
                    max_y: a.max_y.max(b.max_y),
                },
            )
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

fn rasterize_glyph(
    sources: &[Source; 3],
    scaler: &mut Scaler,
    glyph: &Glyph,
    foreground: [u8; 4],
) -> Result<Option<RasterGlyph>> {
    let mut render = Render::new(sources);
    let offset = Vector::new(glyph.x.fract(), glyph.y.fract());
    render.offset(offset).default_color(foreground);
    let Some(image) = render.render(scaler, glyph.id) else {
        return Ok(None);
    };

    let base_x = glyph.x.floor();
    let base_y = glyph.y.floor();
    let left = base_x + image.placement.left as f32;
    let top = base_y - image.placement.top as f32;

    Ok(Some(RasterGlyph { image, left, top }))
}

fn blit_glyph(
    surface: &mut RgbaImage,
    dest_x: i32,
    dest_y: i32,
    glyph: &RasterGlyph,
    color: [u8; 4],
) {
    match glyph.image.content {
        Content::Mask => fill_mask(surface, dest_x, dest_y, glyph, color),
        Content::Color => fill_color(surface, dest_x, dest_y, glyph),
        Content::SubpixelMask => fill_mask(surface, dest_x, dest_y, glyph, color),
    }
}

fn fill_mask(
    surface: &mut RgbaImage,
    dest_x: i32,
    dest_y: i32,
    glyph: &RasterGlyph,
    color: [u8; 4],
) {
    let width = glyph.image.placement.width as i32;
    let height = glyph.image.placement.height as i32;
    if width == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        let y = dest_y + row;
        if y < 0 || y >= surface.height() as i32 {
            continue;
        }
        for col in 0..width {
            let x = dest_x + col;
            if x < 0 || x >= surface.width() as i32 {
                continue;
            }
            let alpha = glyph.image.data[(row * width + col) as usize];
            if alpha == 0 {
                continue;
            }
            let src = tint_color(color, alpha);
            let pixel = surface.get_pixel_mut(x as u32, y as u32);
            blend_pixel(&mut pixel.0, src);
        }
    }
}

fn fill_color(surface: &mut RgbaImage, dest_x: i32, dest_y: i32, glyph: &RasterGlyph) {
    let width = glyph.image.placement.width as i32;
    let height = glyph.image.placement.height as i32;
    if width == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        let y = dest_y + row;
        if y < 0 || y >= surface.height() as i32 {
            continue;
        }
        for col in 0..width {
            let x = dest_x + col;
            if x < 0 || x >= surface.width() as i32 {
                continue;
            }
            let idx = ((row * width + col) * 4) as usize;
            let src = [
                glyph.image.data[idx],
                glyph.image.data[idx + 1],
                glyph.image.data[idx + 2],
                glyph.image.data[idx + 3],
            ];
            if src[3] == 0 {
                continue;
            }
            let pixel = surface.get_pixel_mut(x as u32, y as u32);
            blend_pixel(&mut pixel.0, src);
        }
    }
}

fn tint_color(color: [u8; 4], alpha: u8) -> [u8; 4] {
    let src_a = (u16::from(color[3]) * u16::from(alpha) + 127) / 255;
    [
        ((u16::from(color[0]) * src_a + 127) / 255) as u8,
        ((u16::from(color[1]) * src_a + 127) / 255) as u8,
        ((u16::from(color[2]) * src_a + 127) / 255) as u8,
        src_a as u8,
    ]
}

fn blend_pixel(dst: &mut [u8; 4], src: [u8; 4]) {
    let sa = u16::from(src[3]);
    if sa == 0 {
        return;
    }
    let inv = 255 - sa;
    for i in 0..3 {
        let sc = u16::from(src[i]);
        let dc = u16::from(dst[i]);
        dst[i] = ((sc * sa + dc * inv + 127) / 255) as u8;
    }
    let da = u16::from(dst[3]);
    dst[3] = (sa + (da * inv + 127) / 255).min(255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontBook;
    use crate::layout::{LayoutRequest, Orientation, TextLayouter};
    use crate::types::TextStyle;
    use fontdb::{Family, Query, Stretch, Style, Weight};
    use swash::text::Script;

    fn default_query<'a>(families: &'a [Family<'a>]) -> Query<'a> {
        Query {
            families,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        }
    }

    #[test]
    fn renders_layout_to_image() -> Result<()> {
        let mut book = FontBook::new();
        let mut layouter = TextLayouter::new();
        let families = [Family::SansSerif];
        let font = book
            .query(&default_query(&families))?
            .expect("expected sans-serif font for test");
        let request = LayoutRequest {
            style: TextStyle {
                font: &font,
                font_size: 22.0,
                line_height: 30.0,
                color: [255, 255, 255, 255],
                script: Some(Script::Latin),
            },
            text: "Render test",
            max_primary_axis: 400.0,
            direction: Orientation::Horizontal,
        };
        let result = layouter.layout(&request)?;
        let mut renderer = TextRenderer::new();
        let render_request = RenderRequest {
            style: request.style,
            layout: &result,
            background: [0, 0, 0, 0],
        };
        let rendered = renderer.render(&render_request)?;
        assert!(
            rendered.image.width() > 0 && rendered.image.height() > 0,
            "rendered image should have non-zero dimensions"
        );
        Ok(())
    }
}
