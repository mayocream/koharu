//! Reusable Vello rasterization into CPU-readable images.

use std::sync::mpsc;

use anyhow::{Context, Result, anyhow, bail};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;
use parking_lot::Mutex;
use vello::wgpu::{
    self, Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d,
    TexelCopyBufferInfo, Texture, TextureDescriptor, TextureFormat, TextureUsages, TextureView,
};
use vello::{
    AaConfig, AaSupport, RenderParams, RendererOptions, Scene, kurbo::Affine, peniko::Color,
    util::RenderContext,
};

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

const MAX_SUPERSAMPLING_FACTOR: u32 = 4;

#[derive(Debug)]
pub struct Raster {
    pub image: RgbaImage,
    pub left: i32,
    pub top: i32,
}

struct GpuState {
    context: RenderContext,
    device_id: usize,
    renderer: vello::Renderer,
    targets: Vec<RenderTarget>,
}

struct RenderTarget {
    width: u32,
    height: u32,
    padded_width: u32,
    texture: Texture,
    view: TextureView,
    readback: Buffer,
}

/// A reusable, headless Vello rasterizer.
///
/// GPU setup, Vello's glyph caches, and a small target pool live for the lifetime of this value.
/// Command encoding is serialized because Vello mutates its caches, but GPU completion and
/// readback happen after releasing the lock so independent callers can overlap that work.
pub(crate) struct Rasterizer {
    gpu: Mutex<GpuState>,
}

impl Rasterizer {
    pub(crate) fn new() -> crate::Result<Self> {
        Self::try_new().map_err(crate::Error::Backend)
    }

    fn try_new() -> Result<Self> {
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
                targets: Vec::new(),
            }),
        })
    }

    pub(crate) fn rasterize(
        &self,
        frame: &crate::Frame,
        options: RasterOptions,
    ) -> crate::Result<Raster> {
        let (width, height) = frame.size();
        let (left, top) = frame.origin();
        let image = self
            .rasterize_scene_inner(frame.scene(), width, height, [0, 0, 0, 0], options)
            .map_err(crate::Error::Backend)?;
        Ok(Raster { image, left, top })
    }

    pub(crate) fn rasterize_scene(
        &self,
        scene: &Scene,
        width: u32,
        height: u32,
        background: [u8; 4],
        options: RasterOptions,
    ) -> crate::Result<RgbaImage> {
        self.rasterize_scene_inner(scene, width, height, background, options)
            .map_err(crate::Error::Backend)
    }

    fn rasterize_scene_inner(
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
            let mut gpu = self.gpu.lock();
            let GpuState {
                context,
                device_id,
                renderer,
                targets,
            } = &mut *gpu;
            let device = context.devices[*device_id].device.clone();
            let queue = &context.devices[*device_id].queue;
            let limits = device.limits();
            if width > limits.max_texture_dimension_2d || height > limits.max_texture_dimension_2d {
                bail!(
                    "render surface {width}x{height} exceeds the device limit {}",
                    limits.max_texture_dimension_2d
                );
            }
            let target = targets
                .iter()
                .position(|target| target.width == width && target.height == height)
                .map(|position| targets.swap_remove(position))
                .map_or_else(|| RenderTarget::new(&device, width, height), Ok)?;
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

        let mapped = slice
            .get_mapped_range()
            .context("failed to access mapped WGPU readback buffer")?;
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
        let mut gpu = self.gpu.lock();
        const MAX_POOLED_TARGETS: usize = 4;
        if gpu.targets.len() < MAX_POOLED_TARGETS {
            gpu.targets.push(target);
        }
        Ok(pixels)
    }
}

impl RenderTarget {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Result<Self> {
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
        let row_bytes = width
            .checked_mul(4)
            .context("render target row size overflow")?;
        let padded_width = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer_size = u64::from(padded_width)
            .checked_mul(u64::from(height))
            .context("render target buffer size overflow")?;
        let readback = device.create_buffer(&BufferDescriptor {
            label: Some("koharu text readback"),
            size: buffer_size,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            width,
            height,
            padded_width,
            texture,
            view,
            readback,
        })
    }
}

pub(crate) fn rgba([r, g, b, a]: [u8; 4]) -> Color {
    Color::from_rgba8(r, g, b, a)
}
