use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt as _;

use crate::{Color, PhysicalPoint, PhysicalSize};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    viewport: [f32; 2],
    _padding: [f32; 2],
}

pub(crate) struct OverlayRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    vertices: wgpu::Buffer,
    capacity: usize,
}

impl OverlayRenderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("overlay.wgsl"));
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("koharu canvas overlay uniforms"),
            contents: bytemuck::bytes_of(&Uniforms {
                viewport: [1.0, 1.0],
                _padding: [0.0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("koharu canvas overlay layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("koharu canvas overlay bind group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("koharu canvas overlay pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("koharu canvas overlay pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let capacity = 256;
        let vertices = vertex_buffer(device, capacity);
        Self {
            pipeline,
            bind_group,
            uniform,
            vertices,
            capacity,
        }
    }

    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: PhysicalSize,
        geometry: &OverlayGeometry,
    ) {
        if geometry.vertices.is_empty() || size.is_empty() {
            return;
        }
        if geometry.vertices.len() > self.capacity {
            self.capacity = geometry.vertices.len().next_power_of_two();
            self.vertices = vertex_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&geometry.vertices));
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&Uniforms {
                viewport: [size.width as f32, size.height as f32],
                _padding: [0.0; 2],
            }),
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("koharu canvas overlay pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..geometry.vertices.len() as u32, 0..1);
    }
}

fn vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("koharu canvas overlay vertices"),
        size: (capacity * std::mem::size_of::<Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[derive(Default)]
pub(crate) struct OverlayGeometry {
    vertices: Vec<Vertex>,
}

impl OverlayGeometry {
    pub fn line(&mut self, from: PhysicalPoint, to: PhysicalPoint, width: f64, color: Color) {
        if ![from.x, from.y, to.x, to.y, width]
            .into_iter()
            .all(f64::is_finite)
        {
            return;
        }
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let length = dx.hypot(dy);
        if length <= f64::EPSILON || width <= 0.0 {
            return;
        }
        let x = -dy / length * width * 0.5;
        let y = dx / length * width * 0.5;
        self.quad(
            [
                PhysicalPoint::new(from.x + x, from.y + y),
                PhysicalPoint::new(to.x + x, to.y + y),
                PhysicalPoint::new(to.x - x, to.y - y),
                PhysicalPoint::new(from.x - x, from.y - y),
            ],
            color,
        );
    }

    pub fn dashed_line(
        &mut self,
        from: PhysicalPoint,
        to: PhysicalPoint,
        width: f64,
        dash: f64,
        gap: f64,
        color: Color,
    ) {
        if !dash.is_finite() || dash <= 0.0 || !gap.is_finite() || gap < 0.0 {
            return;
        }
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let length = dx.hypot(dy);
        if length <= f64::EPSILON {
            return;
        }
        let ux = dx / length;
        let uy = dy / length;
        let mut position = 0.0;
        while position < length {
            let end = (position + dash).min(length);
            self.line(
                PhysicalPoint::new(from.x + ux * position, from.y + uy * position),
                PhysicalPoint::new(from.x + ux * end, from.y + uy * end),
                width,
                color,
            );
            position += dash + gap;
        }
    }

    pub fn outline(&mut self, corners: [PhysicalPoint; 4], width: f64, color: Color) {
        for index in 0..4 {
            self.line(corners[index], corners[(index + 1) % 4], width, color);
        }
    }

    pub fn solid_rect(&mut self, center: PhysicalPoint, width: f64, height: f64, color: Color) {
        if ![center.x, center.y, width, height]
            .into_iter()
            .all(f64::is_finite)
            || width <= 0.0
            || height <= 0.0
        {
            return;
        }
        let half_width = width * 0.5;
        let half_height = height * 0.5;
        self.quad(
            [
                PhysicalPoint::new(center.x - half_width, center.y - half_height),
                PhysicalPoint::new(center.x + half_width, center.y - half_height),
                PhysicalPoint::new(center.x + half_width, center.y + half_height),
                PhysicalPoint::new(center.x - half_width, center.y + half_height),
            ],
            color,
        );
    }

    pub fn circle_ring(&mut self, center: PhysicalPoint, radius: f64, width: f64, color: Color) {
        if !radius.is_finite() || radius <= 0.0 || !width.is_finite() || width <= 0.0 {
            return;
        }
        let segments = 48;
        for index in 0..segments {
            let a = std::f64::consts::TAU * index as f64 / segments as f64;
            let b = std::f64::consts::TAU * (index + 1) as f64 / segments as f64;
            self.line(
                PhysicalPoint::new(center.x + radius * a.cos(), center.y + radius * a.sin()),
                PhysicalPoint::new(center.x + radius * b.cos(), center.y + radius * b.sin()),
                width,
                color,
            );
        }
    }

    pub fn label(&mut self, anchor: PhysicalPoint, number: usize, color: Color) {
        let center = PhysicalPoint::new(anchor.x + 7.0, anchor.y - 7.0);
        self.solid_rect(center, 16.0, 16.0, color);
        let text = number.to_string();
        let width = text.len() as f64 * 4.0 - 1.0;
        let mut x = center.x - width * 0.5;
        for digit in text.bytes() {
            self.digit(
                PhysicalPoint::new(x, center.y - 2.5),
                digit,
                [255, 255, 255, 255],
            );
            x += 4.0;
        }
    }

    fn digit(&mut self, origin: PhysicalPoint, digit: u8, color: Color) {
        const DIGITS: [[u8; 5]; 10] = [
            [0b111, 0b101, 0b101, 0b101, 0b111],
            [0b010, 0b110, 0b010, 0b010, 0b111],
            [0b111, 0b001, 0b111, 0b100, 0b111],
            [0b111, 0b001, 0b111, 0b001, 0b111],
            [0b101, 0b101, 0b111, 0b001, 0b001],
            [0b111, 0b100, 0b111, 0b001, 0b111],
            [0b111, 0b100, 0b111, 0b101, 0b111],
            [0b111, 0b001, 0b010, 0b010, 0b010],
            [0b111, 0b101, 0b111, 0b101, 0b111],
            [0b111, 0b101, 0b111, 0b001, 0b111],
        ];
        let Some(rows) = digit
            .checked_sub(b'0')
            .and_then(|value| DIGITS.get(value as usize))
        else {
            return;
        };
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << (2 - column)) != 0 {
                    self.solid_rect(
                        PhysicalPoint::new(origin.x + column as f64, origin.y + row as f64),
                        1.0,
                        1.0,
                        color,
                    );
                }
            }
        }
    }

    fn quad(&mut self, points: [PhysicalPoint; 4], color: Color) {
        let color = color.map(|channel| f32::from(channel) / 255.0);
        for index in [0, 1, 2, 0, 2, 3] {
            self.vertices.push(Vertex {
                position: [points[index].x as f32, points[index].y as f32],
                color,
            });
        }
    }
}
