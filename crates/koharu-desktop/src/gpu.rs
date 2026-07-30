use std::{sync::Arc, time::Instant};

use anyhow::{Context as _, Result, bail};
use koharu_canvas::{Canvas, CanvasGpu, PhysicalSize, ViewState};
use winit::window::Window;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PhysicalRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PhysicalRect {
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_logical(x: f64, y: f64, width: f64, height: f64, dpr: f64) -> Result<Self> {
        if ![x, y, width, height, dpr].into_iter().all(f64::is_finite)
            || x < 0.0
            || y < 0.0
            || width < 0.0
            || height < 0.0
            || dpr <= 0.0
        {
            bail!("viewport coordinates must be finite, non-negative, and use a positive DPR");
        }
        let left = physical_value(x, dpr)?;
        let top = physical_value(y, dpr)?;
        let right = physical_value(x + width, dpr)?;
        let bottom = physical_value(y + height, dpr)?;
        Ok(Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }

    #[must_use]
    pub const fn size(self) -> PhysicalSize {
        PhysicalSize::new(self.width, self.height)
    }

    fn clamp(self, surface: PhysicalSize) -> Self {
        let x = self.x.min(surface.width);
        let y = self.y.min(surface.height);
        Self {
            x,
            y,
            width: self.width.min(surface.width.saturating_sub(x)),
            height: self.height.min(surface.height.saturating_sub(y)),
        }
    }
}

fn physical_value(value: f64, dpr: f64) -> Result<u32> {
    let value = (value * dpr).round();
    if value > f64::from(u32::MAX) {
        bail!("physical viewport coordinate exceeds the supported range");
    }
    Ok(value as u32)
}

pub struct Gpu {
    _instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    adapter_info: wgpu::AdapterInfo,
}

impl Gpu {
    async fn for_window(window: Arc<Window>) -> Result<(Arc<Self>, wgpu::Surface<'static>)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .context("no WGPU adapter supports the desktop surface")?;
        let adapter_info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("koharu desktop device"),
                ..Default::default()
            })
            .await?;
        let gpu = Arc::new(Self {
            _instance: instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
        });
        tracing::info!(adapter = ?gpu.adapter_info, "created desktop WGPU context");
        Ok((gpu, surface))
    }

    #[must_use]
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    #[must_use]
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    #[must_use]
    pub const fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    #[must_use]
    pub fn canvas_gpu(&self) -> CanvasGpu {
        CanvasGpu {
            device: Arc::clone(&self.device),
            queue: Arc::clone(&self.queue),
        }
    }
}

pub(crate) struct Renderer {
    gpu: Arc<Gpu>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    surface_size: PhysicalSize,
    suspended: bool,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bind_group: Option<wgpu::BindGroup>,
    canvas_target_dirty: bool,
    canvas: Canvas,
    view: ViewState,
    viewport: PhysicalRect,
    background: wgpu::Color,
}

pub(crate) struct PresentResult {
    pub needs_redraw: bool,
}

impl Renderer {
    pub async fn new(window: Arc<Window>, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        let initial = window.inner_size();
        let surface_size = PhysicalSize::new(initial.width, initial.height);
        let (gpu, surface) = Gpu::for_window(window).await?;
        let capabilities = surface.get_capabilities(&gpu.adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .context("desktop surface exposes no texture format")?;
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .context("desktop surface exposes no alpha mode")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: surface_size.width.max(1),
            height: surface_size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: Vec::new(),
        };
        surface.configure(&gpu.device, &config);

        let shader = gpu
            .device
            .create_shader_module(wgpu::include_wgsl!("present.wgsl"));
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("koharu desktop canvas layout"),
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
                ],
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("koharu desktop canvas pipeline layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("koharu desktop canvas presenter"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
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
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("koharu desktop canvas sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let canvas = Canvas::new(gpu.canvas_gpu(), wake)?;
        Ok(Self {
            gpu,
            surface,
            config,
            surface_size,
            suspended: surface_size.is_empty(),
            pipeline,
            sampler,
            bind_group: None,
            canvas_target_dirty: true,
            canvas,
            view: ViewState::default(),
            viewport: PhysicalRect::default(),
            background: background_color([245, 245, 245]),
        })
    }

    pub fn gpu(&self) -> &Arc<Gpu> {
        &self.gpu
    }

    pub fn canvas(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    pub const fn view(&self) -> &ViewState {
        &self.view
    }

    pub fn set_view(&mut self, mut view: ViewState) {
        view.size = self.viewport.size();
        self.view = view;
        self.canvas.set_view(self.view.clone());
    }

    pub const fn viewport(&self) -> PhysicalRect {
        self.viewport
    }

    pub fn set_viewport(&mut self, viewport: PhysicalRect) {
        if self.viewport.size() != viewport.size() {
            self.canvas_target_dirty = true;
        }
        self.viewport = viewport;
        self.view.size = viewport.size();
        self.canvas.set_view(self.view.clone());
    }

    pub fn set_background(&mut self, background: [u8; 3]) {
        self.background = background_color(background);
        self.canvas
            .set_workspace_color([background[0], background[1], background[2], 255]);
    }

    pub fn resize_surface(&mut self, size: PhysicalSize) {
        self.surface_size = size;
        self.suspended = size.is_empty();
        if self.suspended {
            return;
        }
        if self.config.width == size.width && self.config.height == size.height {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.gpu.device, &self.config);
    }

    pub fn present(&mut self, now: Instant) -> Result<PresentResult> {
        if self.suspended {
            return Ok(PresentResult {
                needs_redraw: false,
            });
        }
        let frame = self.canvas.render(now)?;
        if self.canvas_target_dirty && !frame.size.is_empty() {
            self.bind_group = Some(
                self.gpu
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("koharu desktop canvas bind group"),
                        layout: &self.pipeline.get_bind_group_layout(0),
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(frame.texture),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.sampler),
                            },
                        ],
                    }),
            );
            self.canvas_target_dirty = false;
        }

        let (surface_texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(PresentResult { needs_redraw: true });
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(PresentResult {
                    needs_redraw: frame.needs_redraw,
                });
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.gpu.device, &self.config);
                return Ok(PresentResult { needs_redraw: true });
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                bail!("desktop surface returned a validation error")
            }
        };
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("koharu desktop present encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("koharu desktop present pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.background),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let viewport = self.viewport.clamp(self.surface_size);
            if let Some(bind_group) = self.bind_group.as_ref()
                && viewport.width != 0
                && viewport.height != 0
                && !frame.size.is_empty()
            {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_viewport(
                    viewport.x as f32,
                    viewport.y as f32,
                    viewport.width as f32,
                    viewport.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(viewport.x, viewport.y, viewport.width, viewport.height);
                pass.draw(0..3, 0..1);
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
        surface_texture.present();
        if suboptimal {
            self.surface.configure(&self.gpu.device, &self.config);
        }
        Ok(PresentResult {
            needs_redraw: frame.needs_redraw,
        })
    }
}

fn background_color([red, green, blue]: [u8; 3]) -> wgpu::Color {
    let linear = |channel: u8| {
        let value = f64::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    wgpu::Color {
        r: linear(red),
        g: linear(green),
        b: linear(blue),
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_viewport_rounds_once_at_the_desktop_boundary() {
        assert_eq!(
            PhysicalRect::from_logical(10.25, 20.5, 300.25, 200.5, 1.5).unwrap(),
            PhysicalRect::new(15, 31, 451, 301)
        );
    }

    #[test]
    fn logical_viewport_rounds_edges_without_a_gap() {
        assert_eq!(
            PhysicalRect::from_logical(0.5, 0.0, 0.5, 1.0, 1.0).unwrap(),
            PhysicalRect::new(1, 0, 0, 1)
        );
    }

    #[test]
    fn invalid_viewports_do_not_reach_wgpu() {
        assert!(PhysicalRect::from_logical(-1.0, 0.0, 10.0, 10.0, 1.0).is_err());
        assert!(PhysicalRect::from_logical(0.0, 0.0, f64::NAN, 10.0, 1.0).is_err());
        assert!(PhysicalRect::from_logical(0.0, 0.0, 10.0, 10.0, 0.0).is_err());
    }

    #[test]
    fn viewport_is_clamped_to_the_surface() {
        assert_eq!(
            PhysicalRect::new(80, 70, 40, 50).clamp(PhysicalSize::new(100, 100)),
            PhysicalRect::new(80, 70, 20, 30)
        );
    }
}
