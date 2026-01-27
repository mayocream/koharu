use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;

use crate::font::Font;
use crate::layout::{LayoutRun, PositionedGlyph, WritingMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum TextShaderEffect {
    #[default]
    Normal,
    Antique,
    Metal,
    Manga,
    MotionBlur,
}

impl TextShaderEffect {
    fn id(self) -> f32 {
        match self {
            Self::Normal => 0.0,
            Self::Antique => 1.0,
            Self::Metal => 2.0,
            Self::Manga => 3.0,
            Self::MotionBlur => 4.0,
        }
    }
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
            effect: TextShaderEffect::Normal,
        }
    }
}

/// WGPU-based text renderer.
pub struct WgpuRenderer {
    context: WgpuContext,
}

impl WgpuRenderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            context: WgpuContext::new()?,
        })
    }

    /// Returns information about the wgpu adapter/device.
    pub fn device_info(&self) -> WgpuDeviceInfo {
        WgpuDeviceInfo::from(&self.context.adapter_info)
    }

    /// Renders the given layout run to an RGBA image.
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

        let mut glyphs_by_font: HashMap<FontKey, FontGlyphs<'_>> = HashMap::new();
        for line in &layout.lines {
            for g in &line.glyphs {
                if let Ok(gid) = u16::try_from(g.glyph_id) {
                    let key = FontKey::of(g.font);
                    let entry = glyphs_by_font.entry(key).or_insert_with(|| FontGlyphs {
                        font: g.font,
                        glyphs: HashSet::new(),
                    });
                    entry.glyphs.insert(gid);
                }
            }
        }

        if glyphs_by_font.is_empty() {
            let bg = opts.background.unwrap_or([0, 0, 0, 0]);
            return Ok(RgbaImage::from_pixel(width, height, image::Rgba(bg)));
        }

        let mut rasters = Vec::new();
        for (key, entry) in glyphs_by_font {
            let fontdue = entry.font.fontdue()?;
            for gid in entry.glyphs {
                let (metrics, mut bitmap) = fontdue.rasterize_indexed(gid, opts.font_size);
                if !opts.anti_alias {
                    for px in &mut bitmap {
                        *px = if *px >= 128 { 255 } else { 0 };
                    }
                }
                rasters.push(RasterizedGlyph {
                    key: FontGlyphId {
                        font: key,
                        glyph: gid,
                    },
                    metrics,
                    bitmap,
                });
            }
        }

        let atlas = GlyphAtlas::new(&self.context.device, &self.context.queue, rasters)?;

        let color = [
            opts.color[0] as f32 / 255.0,
            opts.color[1] as f32 / 255.0,
            opts.color[2] as f32 / 255.0,
            opts.color[3] as f32 / 255.0,
        ];
        let render_uniform = RenderUniform {
            color,
            effect: [opts.effect.id(), opts.font_size, 0.0, 0.0],
        };
        let render_buffer =
            self.context
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("glyph_uniforms"),
                    contents: bytemuck::bytes_of(&render_uniform),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });

        let bind_group = self
            .context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glyph_bind_group"),
                layout: &self.context.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&atlas.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.context.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: render_buffer.as_entire_binding(),
                    },
                ],
            });

        let vertices = build_vertices(
            layout,
            writing_mode,
            &atlas,
            opts.padding,
            width as f32,
            height as f32,
        );
        if vertices.is_empty() {
            let bg = opts.background.unwrap_or([0, 0, 0, 0]);
            return Ok(RgbaImage::from_pixel(width, height, image::Rgba(bg)));
        }

        let vertex_buffer =
            self.context
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("glyph_vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

        let output_texture = self
            .context
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("glyph_output"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bytes_per_row = width * 4;
        let padded_bytes_per_row = align_to(bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let output_buffer_size = padded_bytes_per_row as u64 * height as u64;
        let output_buffer = self.context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glyph_readback"),
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let clear_color = match opts.background {
            Some(bg) => wgpu::Color {
                r: bg[0] as f64 / 255.0,
                g: bg[1] as f64 / 255.0,
                b: bg[2] as f64 / 255.0,
                a: bg[3] as f64 / 255.0,
            },
            None => wgpu::Color::TRANSPARENT,
        };

        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("glyph_render"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glyph_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.context.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(0..vertices.len() as u32, 0..1);
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.context.queue.submit(Some(encoder.finish()));

        let buffer_slice = output_buffer.slice(..);
        let (sender, receiver) = futures::channel::oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.context
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .context("failed to poll wgpu device")?;
        futures::executor::block_on(receiver)
            .context("failed to receive buffer map response")?
            .context("failed to map render buffer")?;

        let mapped = buffer_slice.get_mapped_range();
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height as usize {
            let src_start = y * padded_bytes_per_row as usize;
            let src_end = src_start + bytes_per_row as usize;
            let dst_start = y * (width as usize * 4);
            pixels[dst_start..dst_start + bytes_per_row as usize]
                .copy_from_slice(&mapped[src_start..src_end]);
        }
        drop(mapped);
        output_buffer.unmap();

        let img =
            RgbaImage::from_raw(width, height, pixels).context("failed to build RgbaImage")?;
        Ok(img)
    }
}

/// Information about the wgpu adapter/device.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WgpuDeviceInfo {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
}

impl From<&wgpu::AdapterInfo> for WgpuDeviceInfo {
    fn from(info: &wgpu::AdapterInfo) -> Self {
        Self {
            name: info.name.clone(),
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
            driver: info.driver.clone(),
            driver_info: info.driver_info.clone(),
        }
    }
}

struct WgpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    adapter_info: wgpu::AdapterInfo,
}

impl WgpuContext {
    fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter =
            futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            }))
            .context("failed to request wgpu adapter")?;

        tracing::info!(
            "selected wgpu adapter: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend,
        );

        let (device, queue) =
            futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::default(),
            }))
            .context("failed to request wgpu device")?;

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glyph_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph_shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(GLYPH_SHADER)),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glyph_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Vertex::layout()],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let adapter_info = adapter.get_info();

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            sampler,
            adapter_info,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coord: [f32; 2],
}

impl Vertex {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RenderUniform {
    color: [f32; 4],
    effect: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FontKey(usize);

impl FontKey {
    fn of(font: &Font) -> Self {
        Self(font as *const Font as usize)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FontGlyphId {
    font: FontKey,
    glyph: u16,
}

struct FontGlyphs<'a> {
    font: &'a Font,
    glyphs: HashSet<u16>,
}

struct GlyphAtlas {
    view: wgpu::TextureView,
    glyphs: HashMap<FontGlyphId, AtlasGlyph>,
}

struct AtlasGlyph {
    metrics: fontdue::Metrics,
    uv_min: [f32; 2],
    uv_max: [f32; 2],
}

struct RasterizedGlyph {
    key: FontGlyphId,
    metrics: fontdue::Metrics,
    bitmap: Vec<u8>,
}

impl GlyphAtlas {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rasters: Vec<RasterizedGlyph>,
    ) -> Result<Self> {
        const ATLAS_PADDING: i32 = 1;
        const MIN_ATLAS_SIZE: u32 = 256;
        const MAX_ATLAS_SIZE: u32 = 8192;

        let mut max_dim = 0u32;
        for glyph in &rasters {
            let w = glyph.metrics.width as u32;
            let h = glyph.metrics.height as u32;
            max_dim = max_dim.max(w).max(h);
        }

        let mut atlas_size = MIN_ATLAS_SIZE;
        let padded_max = max_dim.saturating_add((ATLAS_PADDING as u32) * 2);
        while atlas_size < padded_max {
            atlas_size = atlas_size.saturating_mul(2);
        }

        let (atlas_size, allocations) = loop {
            if atlas_size > MAX_ATLAS_SIZE {
                bail!("atlas size exceeded {MAX_ATLAS_SIZE}");
            }

            let mut allocator =
                etagere::AtlasAllocator::new(etagere::size2(atlas_size as i32, atlas_size as i32));
            let mut allocations: HashMap<FontGlyphId, etagere::Allocation> = HashMap::new();
            let mut failed = false;

            for glyph in &rasters {
                if glyph.metrics.width == 0 || glyph.metrics.height == 0 {
                    continue;
                }
                let size = etagere::size2(
                    glyph.metrics.width as i32 + ATLAS_PADDING * 2,
                    glyph.metrics.height as i32 + ATLAS_PADDING * 2,
                );
                let Some(alloc) = allocator.allocate(size) else {
                    failed = true;
                    break;
                };
                allocations.insert(glyph.key, alloc);
            }

            if !failed {
                break (atlas_size, allocations);
            }

            atlas_size = atlas_size.saturating_mul(2);
        };

        let mut atlas_data = vec![0u8; (atlas_size * atlas_size) as usize];
        let mut glyphs = HashMap::with_capacity(rasters.len());

        for glyph in rasters {
            let mut entry = AtlasGlyph {
                metrics: glyph.metrics,
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
            };

            if let Some(alloc) = allocations.get(&glyph.key) {
                let rect = alloc.rectangle;
                let x = rect.min.x + ATLAS_PADDING;
                let y = rect.min.y + ATLAS_PADDING;
                let w = glyph.metrics.width as i32;
                let h = glyph.metrics.height as i32;

                for row in 0..h {
                    let src_start = row as usize * glyph.metrics.width;
                    let src_end = src_start + glyph.metrics.width;
                    let dst_start = (y as usize + row as usize) * atlas_size as usize + x as usize;
                    atlas_data[dst_start..dst_start + glyph.metrics.width]
                        .copy_from_slice(&glyph.bitmap[src_start..src_end]);
                }

                entry.uv_min = [x as f32 / atlas_size as f32, y as f32 / atlas_size as f32];
                entry.uv_max = [
                    (x + w) as f32 / atlas_size as f32,
                    (y + h) as f32 / atlas_size as f32,
                ];
            }

            glyphs.insert(glyph.key, entry);
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas_size),
                rows_per_image: Some(atlas_size),
            },
            wgpu::Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self { view, glyphs })
    }
}

fn build_vertices(
    layout: &LayoutRun<'_>,
    writing_mode: WritingMode,
    atlas: &GlyphAtlas,
    padding: f32,
    width: f32,
    height: f32,
) -> Vec<Vertex> {
    let mut vertices = Vec::new();

    for line in &layout.lines {
        let origin = match writing_mode {
            WritingMode::Horizontal => (padding + line.baseline.0, padding + line.baseline.1),
            WritingMode::VerticalRl => (padding + line.baseline.0, padding + line.baseline.1),
        };
        append_line_vertices(&mut vertices, atlas, &line.glyphs, origin, width, height);
    }

    vertices
}

fn append_line_vertices(
    vertices: &mut Vec<Vertex>,
    atlas: &GlyphAtlas,
    glyphs: &[PositionedGlyph<'_>],
    origin: (f32, f32),
    width: f32,
    height: f32,
) {
    let (origin_x, origin_y) = origin;
    let mut pen_x = 0.0f32;
    let mut pen_y = 0.0f32;

    for g in glyphs {
        let Ok(gid) = u16::try_from(g.glyph_id) else {
            pen_x += g.x_advance;
            pen_y -= g.y_advance;
            continue;
        };

        let entry = match atlas.glyphs.get(&FontGlyphId {
            font: FontKey::of(g.font),
            glyph: gid,
        }) {
            Some(entry) => entry,
            None => {
                pen_x += g.x_advance;
                pen_y -= g.y_advance;
                continue;
            }
        };

        let metrics = &entry.metrics;
        let w = metrics.width as f32;
        let h = metrics.height as f32;

        if w > 0.0 && h > 0.0 {
            let baseline_x = origin_x + pen_x + g.x_offset;
            let baseline_y = origin_y + pen_y - g.y_offset;
            let x = baseline_x + metrics.xmin as f32;
            let y = baseline_y - metrics.ymin as f32 - h;

            let (x0, y0) = to_ndc(x, y, width, height);
            let (x1, y1) = to_ndc(x + w, y + h, width, height);

            let u0 = entry.uv_min[0];
            let v0 = entry.uv_min[1];
            let u1 = entry.uv_max[0];
            let v1 = entry.uv_max[1];

            vertices.extend_from_slice(&[
                Vertex {
                    position: [x0, y0],
                    tex_coord: [u0, v0],
                },
                Vertex {
                    position: [x1, y0],
                    tex_coord: [u1, v0],
                },
                Vertex {
                    position: [x1, y1],
                    tex_coord: [u1, v1],
                },
                Vertex {
                    position: [x0, y0],
                    tex_coord: [u0, v0],
                },
                Vertex {
                    position: [x1, y1],
                    tex_coord: [u1, v1],
                },
                Vertex {
                    position: [x0, y1],
                    tex_coord: [u0, v1],
                },
            ]);
        }

        pen_x += g.x_advance;
        // HarfBuzz/HarfRust positioning uses a Y-up coordinate system; the output is Y-down.
        pen_y -= g.y_advance;
    }
}

fn to_ndc(x: f32, y: f32, width: f32, height: f32) -> (f32, f32) {
    let nx = (x / width) * 2.0 - 1.0;
    let ny = 1.0 - (y / height) * 2.0;
    (nx, ny)
}

fn align_to(value: u32, alignment: u32) -> u32 {
    if alignment == 0 {
        return value;
    }
    value.div_ceil(alignment) * alignment
}

const GLYPH_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coord: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(input.position, 0.0, 1.0);
    out.tex_coord = input.tex_coord;
    return out;
}

@group(0) @binding(0) var glyph_tex: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;
struct RenderUniform {
    color: vec4<f32>,
    effect: vec4<f32>,
};

@group(0) @binding(2) var<uniform> render: RenderUniform;

const EFFECT_NORMAL: f32 = 0.0;
const EFFECT_ANTIQUE: f32 = 1.0;
const EFFECT_METAL: f32 = 2.0;
const EFFECT_MANGA: f32 = 3.0;
const EFFECT_MOTION_BLUR: f32 = 4.0;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453123);
}

fn sample_coverage(uv: vec2<f32>) -> f32 {
    return textureSample(glyph_tex, glyph_sampler, uv).r;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = sample_coverage(input.tex_coord);
    let base_alpha = coverage * render.color.a;
    let base_color = render.color.rgb;
    let effect_id = render.effect.x;
    let frag_pos = input.position.xy;

    var alpha = base_alpha;
    var rgb = base_color;

    if (effect_id < (EFFECT_NORMAL + 0.5)) {
        rgb = base_color;
        alpha = base_alpha;
    } else if (effect_id < (EFFECT_ANTIQUE + 0.5)) {
        let grain = hash(frag_pos * 0.75 + vec2<f32>(render.effect.y * 1.3));
        let blotch = hash(frag_pos * 0.08 + vec2<f32>(11.7, 3.9));
        let fibers = sin(frag_pos.x * 0.045 + frag_pos.y * 0.02) * 0.5 + 0.5;
        let wrinkles = sin(frag_pos.y * 0.03 + grain * 6.0) * 0.5 + 0.5;
        let fade = mix(0.45, 1.0, grain);
        let speckle = step(0.88, grain);
        let stain = smoothstep(0.55, 0.92, blotch);
        let texture = mix(0.75, 1.0, fibers) * mix(0.8, 1.0, wrinkles);
        rgb = base_color * vec3<f32>(1.22, 0.92, 0.62);
        rgb = mix(rgb, rgb * 0.65, stain);
        alpha = base_alpha * fade * texture * (1.0 - speckle * 0.75);
    } else if (effect_id < (EFFECT_METAL + 0.5)) {
        let curve = sin(frag_pos.x * 0.03 + frag_pos.y * 0.008) * 0.5 + 0.5;
        let highlight = pow(smoothstep(0.6, 0.95, curve), 2.4);
        let shadow = mix(0.55, 1.0, curve);
        let brushed = sin(frag_pos.y * 0.25 + frag_pos.x * 0.06) * 0.5 + 0.5;
        let brush = mix(0.9, 1.05, brushed);
        let metallic_base = mix(base_color, vec3<f32>(0.75, 0.78, 0.82), 0.6);
        rgb = metallic_base * shadow * brush;
        rgb = rgb + vec3<f32>(0.95, 0.97, 1.0) * highlight * 0.35;
        alpha = base_alpha;
    } else if (effect_id < (EFFECT_MANGA + 0.5)) {
        let size = clamp(render.effect.y * 0.2, 2.5, 6.0);
        let angle = 0.35;
        let rot = vec2<f32>(
            frag_pos.x * cos(angle) - frag_pos.y * sin(angle),
            frag_pos.x * sin(angle) + frag_pos.y * cos(angle),
        );
        let cell = fract(rot / size) - vec2<f32>(0.5);
        let dist = length(cell);
        let edge = 0.1;
        let aa = fwidth(dist) * 1.5;
        let dot_mask = smoothstep(edge + aa, edge - aa, dist);
        let tone = mix(0.6, 1.0, dot_mask);
        rgb = base_color;
        alpha = base_alpha * tone;
    } else if (effect_id < (EFFECT_MOTION_BLUR + 0.5)) {
        let texel = 1.0 / vec2<f32>(textureDimensions(glyph_tex, 0));
        let dir = normalize(vec2<f32>(1.0, 0.35));
        let spread = texel * 6.0;
        var blur = 0.0;
        blur += sample_coverage(input.tex_coord - dir * spread * 1.5);
        blur += sample_coverage(input.tex_coord - dir * spread * 0.75);
        blur += coverage;
        blur += sample_coverage(input.tex_coord + dir * spread * 0.75);
        blur += sample_coverage(input.tex_coord + dir * spread * 1.5);
        let blurred = blur / 5.0;
        let blur_alpha = blurred * render.color.a;
        rgb = base_color;
        alpha = blur_alpha;
    } else {
        rgb = base_color;
        alpha = base_alpha;
    }

    return vec4<f32>(rgb * alpha, alpha);
}
"#;
