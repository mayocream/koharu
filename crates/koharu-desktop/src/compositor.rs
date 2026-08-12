use std::mem::size_of;

use vello::wgpu;

use crate::{
    browser::{AcceleratedFrame, BrowserFrame, SoftwareFrame},
    geometry::PhysicalRect,
};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Immediates {
    surface_size: [f32; 2],
    viewport_origin: [f32; 2],
    viewport_size: [f32; 2],
    padding: [f32; 2],
}

pub(crate) struct UiTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    sequence: u64,
}

impl UiTexture {
    pub(crate) fn install(
        current: Option<Self>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: BrowserFrame,
    ) -> Self {
        match frame {
            BrowserFrame::Software(frame) => Self::install_software(current, device, queue, &frame),
            BrowserFrame::Accelerated(frame) => Self::install_accelerated(frame),
        }
    }

    fn install_software(
        current: Option<Self>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SoftwareFrame,
    ) -> Self {
        let resized = current.as_ref().is_none_or(|current| {
            current.width != frame.width
                || current.height != frame.height
                || current.format != wgpu::TextureFormat::Bgra8Unorm
        });
        let mut texture =
            current.unwrap_or_else(|| Self::create(device, frame.width, frame.height));
        if resized {
            texture = Self::create(device, frame.width, frame.height);
        }

        if resized {
            texture.write_rect(
                queue,
                frame,
                PhysicalRect {
                    x: 0,
                    y: 0,
                    width: frame.width,
                    height: frame.height,
                },
            );
        } else {
            for rect in &frame.dirty {
                texture.write_rect(
                    queue,
                    frame,
                    PhysicalRect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    },
                );
            }
        }
        texture.sequence = frame.sequence;
        texture
    }

    fn install_accelerated(frame: AcceleratedFrame) -> Self {
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture: frame.texture,
            view,
            width: frame.width,
            height: frame.height,
            format: wgpu::TextureFormat::Rgba8Unorm,
            sequence: frame.sequence,
        }
    }

    fn create(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("koharu windowless browser UI"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width,
            height,
            format: wgpu::TextureFormat::Bgra8Unorm,
            sequence: 0,
        }
    }

    fn write_rect(&self, queue: &wgpu::Queue, frame: &SoftwareFrame, rect: PhysicalRect) {
        let dirty = crate::browser::DirtyRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect.x,
                    y: rect.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            frame.rect_bytes(dirty),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.stride),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: rect.width,
                height: rect.height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub(crate) const fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}

pub(crate) struct Compositor {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    transparent: wgpu::TextureView,
}

pub(crate) struct Composition<'a> {
    pub target: &'a wgpu::TextureView,
    pub surface_size: koharu_canvas::PhysicalSize,
    pub viewport: PhysicalRect,
    pub canvas: Option<&'a wgpu::TextureView>,
    pub ui: Option<&'a wgpu::TextureView>,
}

impl Compositor {
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("koharu desktop texture layout"),
            entries: &[
                texture_layout_entry(0),
                texture_layout_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("koharu desktop compositor pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: size_of::<Immediates>() as u32,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("koharu desktop compositor shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("compositor.wgsl").into()),
        });
        let pipeline = create_pipeline(device, &pipeline_layout, &shader, surface_format);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("koharu desktop compositor sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let transparent = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("koharu desktop transparent fallback"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            layout,
            sampler,
            pipeline,
            transparent,
        }
    }

    pub(crate) fn compose(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        composition: Composition<'_>,
    ) {
        let canvas = composition.canvas.unwrap_or(&self.transparent);
        let ui = composition.ui.unwrap_or(&self.transparent);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("koharu desktop composition bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(canvas),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(ui),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let viewport = composition.viewport.clipped_to(composition.surface_size);
        let immediates = Immediates {
            surface_size: [
                composition.surface_size.width as f32,
                composition.surface_size.height as f32,
            ],
            viewport_origin: [viewport.x as f32, viewport.y as f32],
            viewport_size: [viewport.width as f32, viewport.height as f32],
            padding: [0.0; 2],
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("koharu final desktop composition"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: composition.target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.025,
                        g: 0.025,
                        b: 0.025,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_immediates(0, bytemuck::bytes_of(&immediates));
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn texture_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("koharu desktop compositor"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fragment"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_over_ui_preserves_opaque_output() {
        let source = [0.5_f32, 0.0, 0.0, 0.5];
        let destination = [0.0_f32, 0.5, 0.0, 1.0];
        let output = [
            source[0] * source[3] + destination[0] * destination[3] * (1.0 - source[3]),
            source[1] * source[3] + destination[1] * destination[3] * (1.0 - source[3]),
            source[2] * source[3] + destination[2] * destination[3] * (1.0 - source[3]),
            source[3] + destination[3] * (1.0 - source[3]),
        ];
        assert_eq!(output, [0.25, 0.25, 0.0, 1.0]);
    }
}
