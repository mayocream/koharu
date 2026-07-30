//! Vello scene encoding and reusable headless WGPU rasterization.

use std::sync::{Mutex, mpsc};

use anyhow::{Context, Result, anyhow, bail};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;
use vello::{
    AaConfig, AaSupport, FontEmbolden, Glyph, RenderParams, RendererOptions, Scene,
    kurbo::{Affine, Diagonal2, Join, Stroke},
    peniko::{Color, Fill},
    util::RenderContext,
};
use wgpu::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d,
    TexelCopyBufferInfo, Texture, TextureDescriptor, TextureFormat, TextureUsages, TextureView,
};

use crate::{
    font::font_key,
    layout::{LayoutRun, WritingMode},
};

#[derive(Debug, Clone, Copy)]
pub struct StrokeOptions {
    pub color: [u8; 4],
    pub width_px: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DownsampleFilter {
    Nearest,
    Triangle,
    CatmullRom,
    Gaussian,
    #[default]
    Lanczos3,
}

impl From<DownsampleFilter> for ResizeAlg {
    fn from(value: DownsampleFilter) -> Self {
        match value {
            DownsampleFilter::Nearest => ResizeAlg::Nearest,
            DownsampleFilter::Triangle => ResizeAlg::Convolution(FilterType::Bilinear),
            DownsampleFilter::CatmullRom => ResizeAlg::Convolution(FilterType::CatmullRom),
            DownsampleFilter::Gaussian => ResizeAlg::Convolution(FilterType::Gaussian),
            DownsampleFilter::Lanczos3 => ResizeAlg::Convolution(FilterType::Lanczos3),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterOptions {
    pub supersampling_factor: u32,
    pub downsample_filter: DownsampleFilter,
}

impl RasterOptions {
    #[must_use]
    pub fn supersampled(factor: u32) -> Self {
        Self {
            supersampling_factor: factor,
            ..Default::default()
        }
    }

    fn scale(self) -> u32 {
        self.supersampling_factor.clamp(1, MAX_SUPERSAMPLING_FACTOR)
    }
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            // Vello already uses area anti-aliasing. Supersampling remains available for
            // exports, but should not multiply every interactive render by four by default.
            supersampling_factor: 1,
            downsample_filter: DownsampleFilter::Lanczos3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub color: [u8; 4],
    pub background: Option<[u8; 4]>,
    pub hint_glyphs: bool,
    pub padding: f32,
    pub font_size: f32,
    pub baseline_shift: f32,
    pub stroke: Option<StrokeOptions>,
    pub raster: RasterOptions,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
            background: None,
            hint_glyphs: true,
            padding: 0.0,
            font_size: 16.0,
            baseline_shift: 0.0,
            stroke: None,
            raster: RasterOptions::default(),
        }
    }
}

const MAX_SUPERSAMPLING_FACTOR: u32 = 4;

struct GpuState {
    context: RenderContext,
    device_id: usize,
    renderer: vello::Renderer,
    target: Option<RenderTarget>,
}

struct RenderTarget {
    width: u32,
    height: u32,
    padded_width: u32,
    texture: Texture,
    view: TextureView,
    readback: Buffer,
}

/// A reusable, headless WGPU text renderer.
///
/// GPU setup and Vello's glyph caches live for the lifetime of this value. Rendering is
/// serialized because a Vello renderer mutates those caches; callers may freely share this
/// type between threads instead of creating a device for every text block.
pub struct WgpuRenderer {
    gpu: Mutex<GpuState>,
}

impl WgpuRenderer {
    pub fn new() -> Result<Self> {
        let mut context = RenderContext::new();
        let device_id = pollster::block_on(context.device(None))
            .context("no WGPU adapter supports Vello's required features")?;
        let renderer = vello::Renderer::new(
            &context.devices[device_id].device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                ..Default::default()
            },
        )
        .map_err(|error| anyhow!("failed to create Vello renderer: {error:?}"))?;

        Ok(Self {
            gpu: Mutex::new(GpuState {
                context,
                device_id,
                renderer,
                target: None,
            }),
        })
    }

    pub fn render(
        &self,
        layout: &LayoutRun<'_>,
        writing_mode: WritingMode,
        options: &RenderOptions,
    ) -> Result<RgbaImage> {
        let mut draw_options = options.clone();
        draw_options.padding += options
            .stroke
            .map_or(0.0, |stroke| stroke.width_px.max(0.0));
        let baseline_top = options.baseline_shift.max(0.0);
        let baseline_bottom = (-options.baseline_shift).max(0.0);
        let width = dimension(layout.width, draw_options.padding, "width")?;
        let height = dimension(
            layout.height + baseline_top + baseline_bottom,
            draw_options.padding,
            "height",
        )?;
        let transform = Affine::translate((0.0, baseline_top as f64));
        let mut scene = Scene::new();
        if let Some(stroke) = draw_options
            .stroke
            .filter(|stroke| stroke.width_px > 0.0 && stroke.color[3] > 0)
        {
            draw_layout(
                &mut scene,
                layout,
                writing_mode,
                &draw_options,
                transform,
                DrawStyle::Stroke(stroke),
            );
        }
        draw_layout(
            &mut scene,
            layout,
            writing_mode,
            &draw_options,
            transform,
            DrawStyle::Fill,
        );
        self.rasterize(
            &scene,
            width,
            height,
            options.background.unwrap_or([0, 0, 0, 0]),
            options.raster,
        )
    }

    pub(crate) fn rasterize(
        &self,
        scene: &Scene,
        width: u32,
        height: u32,
        background: [u8; 4],
        raster: RasterOptions,
    ) -> Result<RgbaImage> {
        if width == 0 || height == 0 {
            bail!("invalid render surface {width}x{height}");
        }
        let scale = raster.scale();
        let raster_width = width
            .checked_mul(scale)
            .context("supersampled render surface width overflow")?;
        let raster_height = height
            .checked_mul(scale)
            .context("supersampled render surface height overflow")?;
        let scaled;
        let scene = if scale == 1 {
            scene
        } else {
            scaled = {
                let mut scaled = Scene::new();
                scaled.append(scene, Some(Affine::scale(scale as f64)));
                scaled
            };
            &scaled
        };
        let pixels = self.readback(scene, raster_width, raster_height, background)?;
        let image = RgbaImage::from_raw(raster_width, raster_height, pixels)
            .context("WGPU returned an invalid RGBA buffer")?;
        if scale == 1 {
            return Ok(image);
        }
        let mut downsampled = RgbaImage::new(width, height);
        let resize_options = ResizeOptions::new()
            .resize_alg(raster.downsample_filter.into())
            .use_alpha(true);
        Resizer::new()
            .resize(&image, &mut downsampled, &resize_options)
            .context("failed to downsample WGPU render")?;
        Ok(downsampled)
    }

    fn readback(
        &self,
        scene: &Scene,
        width: u32,
        height: u32,
        background: [u8; 4],
    ) -> Result<Vec<u8>> {
        let size = Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let (device, submission, target) = {
            let mut gpu = self
                .gpu
                .lock()
                .map_err(|_| anyhow!("WGPU renderer lock was poisoned"))?;
            let GpuState {
                context,
                device_id,
                renderer,
                target,
            } = &mut *gpu;
            let device = context.devices[*device_id].device.clone();
            let queue = &context.devices[*device_id].queue;
            let target = target
                .take()
                .filter(|target| target.width == width && target.height == height)
                .unwrap_or_else(|| RenderTarget::new(&device, width, height));
            renderer
                .render_to_texture(
                    &device,
                    queue,
                    scene,
                    &target.view,
                    &RenderParams {
                        base_color: rgba(background),
                        width,
                        height,
                        antialiasing_method: AaConfig::Area,
                    },
                )
                .map_err(|error| anyhow!("Vello rendering failed: {error:?}"))?;

            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("koharu text readback encoder"),
            });
            encoder.copy_texture_to_buffer(
                target.texture.as_image_copy(),
                TexelCopyBufferInfo {
                    buffer: &target.readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(target.padded_width),
                        rows_per_image: None,
                    },
                },
                size,
            );
            let submission = queue.submit([encoder.finish()]);
            (device, submission, target)
        };
        let slice = target.readback.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|error| anyhow!("WGPU device polling failed: {error:?}"))?;
        receiver
            .recv()
            .context("WGPU closed the readback channel")?
            .context("failed to map WGPU readback buffer")?;

        let mapped = slice.get_mapped_range();
        let row_len = (width * 4) as usize;
        let mut pixels = Vec::with_capacity(row_len * height as usize);
        for row in mapped
            .chunks_exact(target.padded_width as usize)
            .take(height as usize)
        {
            pixels.extend_from_slice(&row[..row_len]);
        }
        drop(mapped);
        target.readback.unmap();
        if let Ok(mut gpu) = self.gpu.lock()
            && gpu.target.is_none()
        {
            gpu.target = Some(target);
        }
        Ok(pixels)
    }
}

impl RenderTarget {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let size = Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("koharu text target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let padded_width = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = device.create_buffer(&BufferDescriptor {
            label: Some("koharu text readback"),
            size: u64::from(padded_width) * u64::from(height),
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            width,
            height,
            padded_width,
            texture,
            view,
            readback,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DrawStyle {
    Stroke(StrokeOptions),
    Fill,
}

pub(crate) fn draw_layout(
    scene: &mut Scene,
    layout: &LayoutRun<'_>,
    writing_mode: WritingMode,
    options: &RenderOptions,
    transform: Affine,
    style: DrawStyle,
) {
    for line in &layout.lines {
        let (baseline_x, baseline_y) = match writing_mode {
            WritingMode::Horizontal | WritingMode::VerticalRl | WritingMode::VerticalLr => {
                line.baseline
            }
        };
        let mut pen_x = 0.0;
        let mut pen_y = 0.0;
        let mut start = 0;

        while start < line.glyphs.len() {
            let font = line.glyphs[start].font;
            let key = font_key(font);
            let mut end = start + 1;
            while end < line.glyphs.len() && font_key(line.glyphs[end].font) == key {
                end += 1;
            }

            let mut glyphs = Vec::with_capacity(end - start);
            for glyph in &line.glyphs[start..end] {
                glyphs.push(Glyph {
                    id: glyph.glyph_id,
                    x: options.padding + baseline_x + pen_x + glyph.x_offset,
                    y: options.padding + baseline_y + pen_y
                        - glyph.y_offset
                        - options.baseline_shift,
                });
                pen_x += glyph.x_advance;
                pen_y -= glyph.y_advance;
            }

            let font_data = font.vello_data();
            let normalized_coords = font.normalized_coords();
            let mut run = scene
                .draw_glyphs(&font_data)
                .font_size(options.font_size)
                .transform(transform)
                .hint(options.hint_glyphs);
            if !normalized_coords.is_empty() {
                run = run.normalized_coords(normalized_coords);
            }
            if let Some(angle) = font.synthetic_skew() {
                run = run
                    .glyph_transform(Some(Affine::skew(-(angle.to_radians().tan() as f64), 0.0)));
            }
            if font.synthetic_bold() {
                run = run.font_embolden(FontEmbolden::new(Diagonal2::new(1.0, 1.0)));
            }

            match style {
                DrawStyle::Fill => run
                    .brush(rgba(options.color))
                    .draw(Fill::NonZero, glyphs.into_iter()),
                DrawStyle::Stroke(stroke) => {
                    let outline =
                        Stroke::new((stroke.width_px * 2.0) as f64).with_join(Join::Round);
                    run.brush(rgba(stroke.color))
                        .draw(&outline, glyphs.into_iter());
                }
            }
            start = end;
        }
    }
}

fn dimension(content: f32, padding: f32, name: &str) -> Result<u32> {
    let value = content + padding * 2.0;
    if !value.is_finite() || value <= 0.0 || value > u32::MAX as f32 {
        bail!("invalid render surface {name}: {value}");
    }
    Ok(value.ceil() as u32)
}

fn rgba([r, g, b, a]: [u8; 4]) -> Color {
    Color::from_rgba8(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_surface_dimensions_without_a_device() {
        assert_eq!(dimension(12.1, 2.0, "width").unwrap(), 17);
        assert!(dimension(0.0, 0.0, "width").is_err());
        assert!(dimension(f32::NAN, 0.0, "width").is_err());
    }
}
