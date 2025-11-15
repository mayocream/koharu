use anyhow::Result;
use image::{Rgba, RgbaImage};
use swash::scale::image::{Content, Image as GlyphImage};
use swash::scale::{Render, ScaleContext, Scaler, Source, StrikeWith};
use swash::shape::cluster::Glyph;
use swash::zeno::Vector;

use crate::font::Font;
use crate::layout::LayoutResult;

/// Describes the input needed to paint previously laid out text.
pub struct RenderRequest<'a> {
    pub font: &'a Font,
    pub layout: &'a LayoutResult,
    pub foreground: [u8; 4],
    pub background: [u8; 4],
}

/// Final rasterized text image and the origin offset that was applied.
pub struct RenderedText {
    pub image: RgbaImage,
    pub origin: (f32, f32),
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

    fn include(self, glyph: &RasterGlyph) -> Self {
        let right = glyph.left + glyph.image.placement.width as f32;
        let bottom = glyph.top + glyph.image.placement.height as f32;
        Self {
            min_x: self.min_x.min(glyph.left),
            min_y: self.min_y.min(glyph.top),
            max_x: self.max_x.max(right),
            max_y: self.max_y.max(bottom),
        }
    }
}

/// Rasterizes glyph runs produced by [`TextLayouter`](crate::layout::TextLayouter).
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
        if request.layout.lines.is_empty() {
            let image = RgbaImage::from_pixel(1, 1, Rgba(request.background));
            return Ok(RenderedText {
                image,
                origin: (0.0, 0.0),
            });
        }

        let font_ref = request.font.font_ref()?;
        let mut scaler = self
            .scale_context
            .builder(font_ref)
            .size(request.layout.font_size.max(0.0))
            .build();

        let mut rasterized = Vec::new();
        let mut bounds: Option<GlyphBounds> = None;

        for line in &request.layout.lines {
            for glyph in &line.glyphs {
                if let Some(raster_glyph) =
                    rasterize_glyph(&self.sources, &mut scaler, glyph, request.foreground)?
                {
                    bounds = Some(match bounds {
                        Some(existing) => existing.include(&raster_glyph),
                        None => GlyphBounds::from_glyph(&raster_glyph),
                    });
                    rasterized.push(raster_glyph);
                }
            }
        }

        let Some(bounds) = bounds else {
            let image = RgbaImage::from_pixel(1, 1, Rgba(request.background));
            return Ok(RenderedText {
                image,
                origin: (0.0, 0.0),
            });
        };

        let width = (bounds.max_x - bounds.min_x).ceil().max(1.0) as u32;
        let height = (bounds.max_y - bounds.min_y).ceil().max(1.0) as u32;
        let mut surface = RgbaImage::from_pixel(width, height, Rgba(request.background));

        for glyph in &rasterized {
            let dest_x = (glyph.left - bounds.min_x).floor() as i32;
            let dest_y = (glyph.top - bounds.min_y).floor() as i32;
            blit_glyph(&mut surface, dest_x, dest_y, glyph, request.foreground);
        }

        Ok(RenderedText {
            image: surface,
            origin: (bounds.min_x, bounds.min_y),
        })
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
    use crate::layout::{LayoutRequest, Orientation, LayoutSession, TextLayouter};
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
        let options = LayoutRequest {
            text: "Render test",
            font_query: default_query(&families),
            script: Some(Script::Latin),
            font_size: 22.0,
            max_primary_axis: 400.0,
            line_height: 30.0,
            direction: Orientation::Horizontal,
        };
        let LayoutSession { font, output } = layouter.layout(&mut book, &options)?;
        let mut renderer = TextRenderer::new();
        let request = RenderRequest {
            font: &font,
            layout: &output,
            foreground: [255, 255, 255, 255],
            background: [0, 0, 0, 0],
        };
        let rendered = renderer.render(&request)?;
        assert!(
            rendered.image.width() > 0 && rendered.image.height() > 0,
            "rendered image should have non-zero dimensions"
        );
        Ok(())
    }
}
