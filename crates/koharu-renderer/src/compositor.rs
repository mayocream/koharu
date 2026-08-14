//! Ordered GPU composition for decoded raster images and Vello vector batches.

use std::collections::{HashMap, HashSet};

use anyhow::anyhow;
use koharu_scene::BlobId;
use vello::{
    AaConfig, RenderParams, Scene,
    kurbo::Affine,
    peniko::Color,
    wgpu::{self, util::DeviceExt as _},
};

use crate::{Error, RasterImage, Result};

const SHADER: &str = r#"
struct DrawUniforms {
    linear: vec4<f32>,
    translation_source: vec4<f32>,
    target_options: vec4<f32>,
    format_options: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) screen_uv: vec2<f32>,
}

@group(0) @binding(0)
var source_texture: texture_2d<f32>;
@group(0) @binding(1)
var erase_texture: texture_2d<f32>;
@group(0) @binding(2)
var texture_sampler: sampler;
@group(0) @binding(3)
var<uniform> uniforms: DrawUniforms;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let coordinates = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let uv = coordinates[index];
    let source = uv * uniforms.translation_source.zw;
    let screen = vec2<f32>(
        uniforms.linear.x * source.x + uniforms.linear.z * source.y,
        uniforms.linear.y * source.x + uniforms.linear.w * source.y,
    ) + uniforms.translation_source.xy;
    let target_size = uniforms.target_options.xy;
    var output: VertexOutput;
    output.position = vec4<f32>(
        screen.x / target_size.x * 2.0 - 1.0,
        1.0 - screen.y / target_size.y * 2.0,
        0.0,
        1.0,
    );
    output.uv = uv;
    output.screen_uv = screen / target_size;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    var color: vec4<f32>;
    if uniforms.format_options.y > 0.0 {
        let dimensions = textureDimensions(source_texture);
        let coordinate = clamp(
            vec2<i32>(floor(input.uv * vec2<f32>(dimensions))),
            vec2<i32>(0),
            vec2<i32>(dimensions) - vec2<i32>(1),
        );
        color = textureLoad(source_texture, coordinate, 0);
    } else {
        color = textureSample(source_texture, texture_sampler, input.uv);
    }
    var erase = 0.0;
    if uniforms.target_options.w > 0.0 {
        erase = textureSample(erase_texture, texture_sampler, input.screen_uv).a;
    }
    if uniforms.format_options.x == 0.0 {
        color = vec4<f32>(color.rgb * color.a, color.a);
    }
    let opacity = uniforms.target_options.z;
    return color * (opacity * (1.0 - erase * uniforms.target_options.w));
}
"#;

pub enum CompositionCommand {
    Raster(RasterDraw),
    Vector(Scene),
}

pub struct RasterDraw {
    pub image: RasterImage,
    pub transform: Affine,
    pub opacity: f32,
    pub erase: bool,
}

struct CachedTexture {
    width: u32,
    height: u32,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct ScratchTarget {
    size: (u32, u32),
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl ScratchTarget {
    fn new(device: &wgpu::Device, label: &str, size: (u32, u32)) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            size,
            _texture: texture,
            view,
        }
    }
}

pub struct GpuCompositor {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    empty_mask: CachedTexture,
    overlay: Option<ScratchTarget>,
    erase: Option<ScratchTarget>,
    images: HashMap<BlobId, CachedTexture>,
}

impl GpuCompositor {
    #[must_use]
    pub fn new(device: &wgpu::Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("koharu raster compositor sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("koharu raster compositor bind group layout"),
            entries: &[
                texture_layout(0),
                texture_layout(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("koharu raster compositor pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("koharu raster compositor shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("koharu raster compositor pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
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
        let empty_mask = create_texture(device, "koharu empty erase mask", 1, 1);
        Self {
            pipeline,
            bind_group_layout,
            sampler,
            empty_mask,
            overlay: None,
            erase: None,
            images: HashMap::new(),
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vello: &mut vello::Renderer,
        target: &wgpu::TextureView,
        size: (u32, u32),
        commands: &[CompositionCommand],
        erase_mask: Option<&Scene>,
        background: [u8; 4],
        clip: [u32; 4],
    ) -> Result<()> {
        if size.0 == 0 || size.1 == 0 {
            return Ok(());
        }
        if commands
            .iter()
            .any(|command| matches!(command, CompositionCommand::Vector(_)))
            && self
                .overlay
                .as_ref()
                .is_none_or(|target| target.size != size)
        {
            self.overlay = Some(ScratchTarget::new(device, "koharu vector overlay", size));
        }
        if erase_mask.is_some() && self.erase.as_ref().is_none_or(|target| target.size != size) {
            self.erase = Some(ScratchTarget::new(device, "koharu erase overlay", size));
        }
        let active = commands
            .iter()
            .filter_map(|command| match command {
                CompositionCommand::Raster(draw) => Some(draw.image.blob()),
                CompositionCommand::Vector(_) => None,
            })
            .collect::<HashSet<_>>();
        self.images.retain(|blob, _| active.contains(blob));
        for command in commands {
            if let CompositionCommand::Raster(draw) = command {
                self.upload(device, queue, &draw.image)?;
            }
        }

        if let Some(mask) = erase_mask {
            render_vello(
                vello,
                device,
                queue,
                mask,
                &self
                    .erase
                    .as_ref()
                    .expect("erase target created above")
                    .view,
                size,
            )?;
        }
        clear_target(device, queue, target, background);
        let clip = clipped_rect(clip, size);
        if clip[2] == 0 || clip[3] == 0 {
            return Ok(());
        }
        for command in commands {
            match command {
                CompositionCommand::Raster(draw) => {
                    let source = &self.images[&draw.image.blob()].view;
                    let erase = if draw.erase && erase_mask.is_some() {
                        &self
                            .erase
                            .as_ref()
                            .expect("erase target created above")
                            .view
                    } else {
                        &self.empty_mask.view
                    };
                    self.draw(
                        device,
                        queue,
                        source,
                        erase,
                        target,
                        draw.transform,
                        draw.image.size(),
                        size,
                        draw.opacity.clamp(0.0, 1.0),
                        draw.erase && erase_mask.is_some(),
                        false,
                        clip,
                    );
                }
                CompositionCommand::Vector(scene) => {
                    let overlay = &self
                        .overlay
                        .as_ref()
                        .expect("vector target created above")
                        .view;
                    render_vello(vello, device, queue, scene, overlay, size)?;
                    self.draw(
                        device,
                        queue,
                        overlay,
                        &self.empty_mask.view,
                        target,
                        Affine::IDENTITY,
                        size,
                        size,
                        1.0,
                        false,
                        true,
                        clip,
                    );
                }
            }
        }
        Ok(())
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &RasterImage,
    ) -> Result<()> {
        let (width, height) = image.size();
        let expected = usize::try_from(u64::from(width) * u64::from(height) * 4)
            .map_err(|_| Error::invalid("raster image byte length exceeds usize"))?;
        if width == 0 || height == 0 || image.pixels().len() != expected {
            return Err(Error::invalid(format!(
                "raster image {} has invalid dimensions or byte length",
                image.blob()
            )));
        }
        let limit = device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return Err(Error::invalid(format!(
                "raster image {width}x{height} exceeds the device limit {limit}"
            )));
        }
        if self
            .images
            .get(&image.blob())
            .is_some_and(|cached| cached.width == width && cached.height == height)
        {
            return Ok(());
        }
        let texture = create_texture(device, "koharu decoded raster image", width, height);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture._texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.images.insert(image.blob(), texture);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::TextureView,
        erase: &wgpu::TextureView,
        target: &wgpu::TextureView,
        transform: Affine,
        source_size: (u32, u32),
        target_size: (u32, u32),
        opacity: f32,
        use_erase: bool,
        premultiplied: bool,
        clip: [u32; 4],
    ) {
        let [a, b, c, d, e, f] = transform.as_coeffs();
        let pixel_aligned =
            a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0 && e.fract() == 0.0 && f.fract() == 0.0;
        let values = [
            a as f32,
            b as f32,
            c as f32,
            d as f32,
            e as f32,
            f as f32,
            source_size.0 as f32,
            source_size.1 as f32,
            target_size.0 as f32,
            target_size.1 as f32,
            opacity,
            f32::from(use_erase),
            f32::from(premultiplied),
            f32::from(pixel_aligned),
            0.0,
            0.0,
        ];
        let bytes = values
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect::<Vec<_>>();
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("koharu raster compositor uniforms"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("koharu raster compositor bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(erase),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: uniforms.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("koharu raster compositor encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("koharu raster compositor pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}

fn texture_layout(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn create_texture(device: &wgpu::Device, label: &str, width: u32, height: u32) -> CachedTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    CachedTexture {
        width,
        height,
        _texture: texture,
        view,
    }
}

fn render_vello(
    renderer: &mut vello::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    target: &wgpu::TextureView,
    size: (u32, u32),
) -> Result<()> {
    renderer
        .render_to_texture(
            device,
            queue,
            scene,
            target,
            &RenderParams {
                base_color: Color::TRANSPARENT,
                width: size.0,
                height: size.1,
                antialiasing_method: AaConfig::Area,
            },
        )
        .map_err(|error| Error::Backend(anyhow!("Vello rendering failed: {error:?}")))
}

fn clear_target(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &wgpu::TextureView,
    [r, g, b, a]: [u8; 4],
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("koharu raster compositor clear encoder"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("koharu raster compositor clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from(r) / 255.0,
                        g: f64::from(g) / 255.0,
                        b: f64::from(b) / 255.0,
                        a: f64::from(a) / 255.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    queue.submit([encoder.finish()]);
}

fn clipped_rect([x, y, width, height]: [u32; 4], size: (u32, u32)) -> [u32; 4] {
    let x = x.min(size.0);
    let y = y.min(size.1);
    [
        x,
        y,
        width.min(size.0.saturating_sub(x)),
        height.min(size.1.saturating_sub(y)),
    ]
}
