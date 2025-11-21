use anyhow::Result;
use image::{Pixel, Rgba, RgbaImage};
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Format, Vector};

use crate::layout::LayoutResult;
use crate::types::Color;

#[derive(Debug)]
pub struct RenderRequest<'a> {
    pub layout: &'a LayoutResult,
    pub image: &'a mut RgbaImage,
    pub x: f32,
    pub y: f32,
    pub font_size: f32,
    pub color: Color,
}

pub struct Renderer {
    scale_context: ScaleContext,
    sources: [Source; 3],
}

impl Renderer {
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

    pub fn render(&mut self, request: &mut RenderRequest) -> Result<()> {
        for line in request.layout {
            let font = line.font.font_ref()?;
            let mut scaler = self
                .scale_context
                .builder(font)
                .size(request.font_size)
                .hint(true)
                .build();

            let baseline_x = line.baseline.0 + request.x;
            let baseline_y = line.baseline.1 + request.y;

            for glyph in &line.glyphs {
                let glyph_x = baseline_x + glyph.x;
                let glyph_y = baseline_y + glyph.y;

                if let Some(rendered) = Render::new(&self.sources)
                    .format(Format::Subpixel)
                    .offset(Vector::new(glyph_x.fract(), glyph_y.fract()))
                    .render(&mut scaler, glyph.id)
                {
                    blit_glyph(
                        &mut request.image,
                        &rendered,
                        glyph_x.floor() as i32,
                        glyph_y.floor() as i32,
                        request.color,
                    );
                }
            }
        }

        Ok(())
    }
}

/// Blit a rendered glyph onto the image
fn blit_glyph(
    image: &mut RgbaImage,
    glyph_image: &swash::scale::image::Image,
    x: i32,
    y: i32,
    color: Color,
) {
    let placement = glyph_image.placement;
    let glyph_x = x + placement.left;
    let glyph_y = y - placement.top;

    match glyph_image.content {
        Content::Mask => blit_mask(image, glyph_image, glyph_x, glyph_y, color),
        Content::SubpixelMask => blit_subpixel(image, glyph_image, glyph_x, glyph_y, color),
        Content::Color => blit_color(image, glyph_image, glyph_x, glyph_y),
    }
}

fn blit_mask(
    image: &mut RgbaImage,
    glyph_image: &swash::scale::image::Image,
    glyph_x: i32,
    glyph_y: i32,
    color: Color,
) {
    visit_glyph_pixels(
        image,
        glyph_image,
        glyph_x,
        glyph_y,
        1,
        |pixel, src_data| {
            blend_alpha(pixel, src_data, color);
        },
    );
}

fn blit_subpixel(
    image: &mut RgbaImage,
    glyph_image: &swash::scale::image::Image,
    glyph_x: i32,
    glyph_y: i32,
    color: Color,
) {
    visit_glyph_pixels(
        image,
        glyph_image,
        glyph_x,
        glyph_y,
        4,
        |pixel, src_data| {
            blend_subpixel(pixel, src_data, color);
        },
    );
}

fn blit_color(
    image: &mut RgbaImage,
    glyph_image: &swash::scale::image::Image,
    glyph_x: i32,
    glyph_y: i32,
) {
    visit_glyph_pixels(
        image,
        glyph_image,
        glyph_x,
        glyph_y,
        4,
        |pixel, src_data| {
            blend_color(pixel, src_data);
        },
    );
}

fn visit_glyph_pixels<F>(
    image: &mut RgbaImage,
    glyph_image: &swash::scale::image::Image,
    glyph_x: i32,
    glyph_y: i32,
    bytes_per_pixel: usize,
    mut f: F,
) where
    F: FnMut(&mut Rgba<u8>, &[u8]),
{
    let placement = glyph_image.placement;
    let width = placement.width as usize;
    let height = placement.height as usize;
    if width == 0 || height == 0 {
        return;
    }

    let (image_width, image_height) = image.dimensions();
    for dy in 0..height {
        let dst_y = glyph_y + dy as i32;
        if dst_y < 0 || dst_y >= image_height as i32 {
            continue;
        }

        for dx in 0..width {
            let dst_x = glyph_x + dx as i32;
            if dst_x < 0 || dst_x >= image_width as i32 {
                continue;
            }

            let src_idx = (dy * width + dx) * bytes_per_pixel;
            if src_idx + bytes_per_pixel > glyph_image.data.len() {
                continue;
            }

            let src_data = &glyph_image.data[src_idx..src_idx + bytes_per_pixel];
            let pixel = image.get_pixel_mut(dst_x as u32, dst_y as u32);
            f(pixel, src_data);
        }
    }
}

/// Blend alpha mask with text color
#[inline]
fn blend_alpha(pixel: &mut Rgba<u8>, src_data: &[u8], color: Color) {
    let coverage = src_data[0] as u32;
    if coverage == 0 {
        return;
    }

    let tinted = [
        ((color[0] as u32 * coverage) / 255) as u8,
        ((color[1] as u32 * coverage) / 255) as u8,
        ((color[2] as u32 * coverage) / 255) as u8,
        ((color[3] as u32 * coverage) / 255) as u8,
    ];
    composite_pixel(pixel, tinted);
}

/// Blend subpixel mask with text color
#[inline]
fn blend_subpixel(pixel: &mut Rgba<u8>, src_data: &[u8], color: Color) {
    let r = src_data[0] as u32;
    let g = src_data[1] as u32;
    let b = src_data[2] as u32;
    let stored_alpha = *src_data.get(3).unwrap_or(&0) as u32;
    if r == 0 && g == 0 && b == 0 && stored_alpha == 0 {
        return;
    }
    let coverage_alpha = if stored_alpha != 0 {
        stored_alpha
    } else {
        (r + g + b) / 3
    };

    let tinted = [
        ((color[0] as u32 * r) / 255) as u8,
        ((color[1] as u32 * g) / 255) as u8,
        ((color[2] as u32 * b) / 255) as u8,
        ((color[3] as u32 * coverage_alpha) / 255) as u8,
    ];
    composite_pixel(pixel, tinted);
}

/// Blend color bitmap
#[inline]
fn blend_color(pixel: &mut Rgba<u8>, src_data: &[u8]) {
    let alpha = src_data[3];
    if alpha == 0 {
        return;
    }
    composite_pixel(pixel, [src_data[0], src_data[1], src_data[2], alpha]);
}

#[inline]
fn composite_pixel(pixel: &mut Rgba<u8>, src: [u8; 4]) {
    let alpha = src[3] as u32;
    if alpha == 0 {
        return;
    }
    let inv_alpha = 255 - alpha;
    let channels = pixel.channels_mut();
    channels[0] = ((inv_alpha * channels[0] as u32 + alpha * src[0] as u32) / 255) as u8;
    channels[1] = ((inv_alpha * channels[1] as u32 + alpha * src[1] as u32) / 255) as u8;
    channels[2] = ((inv_alpha * channels[2] as u32 + alpha * src[2] as u32) / 255) as u8;
    channels[3] = ((inv_alpha * channels[3] as u32 + alpha * 255) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(value: [u8; 4]) -> Rgba<u8> {
        Rgba(value)
    }

    #[test]
    fn blend_alpha_applies_full_coverage() {
        let mut pixel = rgba([0, 0, 0, 0]);
        blend_alpha(&mut pixel, &[255], [10, 20, 30, 255]);
        assert_eq!(pixel.0, [10, 20, 30, 255]);
    }

    #[test]
    fn blend_alpha_noops_on_zero_coverage() {
        let mut pixel = rgba([1, 2, 3, 4]);
        blend_alpha(&mut pixel, &[0], [200, 200, 200, 200]);
        assert_eq!(pixel.0, [1, 2, 3, 4]);
    }

    #[test]
    fn blend_subpixel_uses_rgb_coverage_when_alpha_missing() {
        let mut pixel = rgba([0, 0, 0, 0]);
        blend_subpixel(&mut pixel, &[64, 128, 255, 0], [255, 255, 255, 255]);
        assert_eq!(pixel.0, [37, 74, 149, 149]); // avg alpha of RGB channels
    }

    #[test]
    fn blend_subpixel_prefers_stored_alpha() {
        let mut pixel = rgba([0, 0, 0, 0]);
        blend_subpixel(&mut pixel, &[10, 20, 30, 200], [255, 255, 255, 255]);
        assert_eq!(pixel.0, [7, 15, 23, 200]);
    }

    #[test]
    fn blend_subpixel_respects_text_color() {
        let mut pixel = rgba([0, 0, 0, 0]);
        blend_subpixel(&mut pixel, &[128, 64, 32, 0], [100, 150, 200, 255]);
        assert_eq!(pixel.0, [14, 10, 7, 74]);
    }

    #[test]
    fn blend_subpixel_composites_with_existing_color() {
        let mut pixel = rgba([200, 200, 200, 255]);
        blend_subpixel(&mut pixel, &[255, 0, 0, 128], [255, 255, 255, 255]);
        assert_eq!(pixel.0, [227, 99, 99, 255]);
    }

    #[test]
    fn blend_color_copies_src_when_opaque() {
        let mut pixel = rgba([0, 0, 0, 0]);
        blend_color(&mut pixel, &[5, 6, 7, 255]);
        assert_eq!(pixel.0, [5, 6, 7, 255]);
    }

    #[test]
    fn blend_color_ignores_transparent_src() {
        let mut pixel = rgba([9, 9, 9, 9]);
        blend_color(&mut pixel, &[5, 6, 7, 0]);
        assert_eq!(pixel.0, [9, 9, 9, 9]);
    }
}
