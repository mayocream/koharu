//! WGPU surface presentation for the native Tauri window.

use std::{cmp::Reverse, sync::Arc, time::Instant};

use anyhow::{Context as _, Result, bail};
use koharu_canvas::{Canvas, CanvasGpu, PhysicalPoint, PhysicalSize, ViewState};
use tauri::{Runtime, WebviewWindow};
use vello::wgpu::{self, util::TextureBlitter};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PhysicalRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PhysicalRect {
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
}

fn physical_value(value: f64, dpr: f64) -> Result<u32> {
    let value = (value * dpr).round();
    if value > f64::from(u32::MAX) {
        bail!("physical viewport coordinate exceeds the supported range");
    }
    Ok(value as u32)
}

struct DesktopGpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    canvas: Canvas,
}

impl DesktopGpu {
    async fn select(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        surface_size: PhysicalSize,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let mut adapters = instance
            .enumerate_adapters(desktop_backends())
            .await
            .into_iter()
            .filter(|adapter| adapter.is_surface_supported(surface))
            .map(|adapter| (adapter.get_info(), adapter))
            .filter(|(info, _)| info.device_type != wgpu::DeviceType::Cpu)
            .collect::<Vec<_>>();
        adapters.sort_by_key(|(info, _)| Reverse(adapter_priority(info.device_type, info.backend)));

        let mut failures = Vec::new();
        for (info, adapter) in adapters {
            match Self::initialize(adapter, surface, surface_size, Arc::clone(&wake)).await {
                Ok(gpu) => {
                    tracing::info!(adapter = ?info, "created desktop WGPU context");
                    return Ok(gpu);
                }
                Err(error) => {
                    tracing::warn!(adapter = ?info, error = ?error, "rejected desktop WGPU adapter");
                    failures.push(format!("{} ({}): {error:#}", info.name, info.backend));
                }
            }
        }

        let reason = match failures.as_slice() {
            [] => "no GPU supports the desktop surface".to_owned(),
            _ => failures.join("; "),
        };
        bail!("no compatible GPU could initialize the desktop renderer: {reason}")
    }

    async fn initialize(
        adapter: wgpu::Adapter,
        surface: &wgpu::Surface<'_>,
        surface_size: PhysicalSize,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| {
                matches!(
                    format,
                    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
                )
            })
            .or_else(|| capabilities.formats.first().copied())
            .context("surface exposes no texture format")?;
        let alpha_mode = capabilities
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
            .or_else(|| capabilities.alpha_modes.first().copied())
            .context("surface exposes no alpha mode")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("koharu desktop device"),
                ..Default::default()
            })
            .await
            .context("failed to create the WGPU device")?;
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let mut canvas = Canvas::new(
            CanvasGpu {
                device: Arc::clone(&device),
                queue: Arc::clone(&queue),
            },
            wake,
        )
        .context("failed to create the canvas renderer")?;
        canvas.set_render_target(surface_size, PhysicalPoint::default());

        Ok(Self {
            device,
            queue,
            format,
            alpha_mode,
            canvas,
        })
    }
}

pub(crate) struct Renderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    surface_size: PhysicalSize,
    blitter: TextureBlitter,
    canvas: Canvas,
    viewport: PhysicalRect,
}

impl Renderer {
    pub async fn new<R: Runtime>(
        window: WebviewWindow<R>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let initial = window.inner_size()?;
        let surface_size = PhysicalSize::new(initial.width, initial.height);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: desktop_backends(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(window)?;
        let gpu = DesktopGpu::select(&instance, &surface, surface_size, wake).await?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: gpu.format,
            width: surface_size.width.max(1),
            height: surface_size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: gpu.alpha_mode,
            view_formats: Vec::new(),
        };
        surface.configure(&gpu.device, &config);
        let blitter = TextureBlitter::new(&gpu.device, gpu.format);
        let mut renderer = Self {
            device: gpu.device,
            queue: gpu.queue,
            surface,
            config,
            surface_size,
            blitter,
            canvas: gpu.canvas,
            viewport: PhysicalRect::default(),
        };
        renderer.present(Instant::now(), surface_size)?;
        Ok(renderer)
    }

    pub fn canvas(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    pub const fn canvas_ref(&self) -> &Canvas {
        &self.canvas
    }

    pub const fn view(&self) -> &ViewState {
        self.canvas.view()
    }

    pub fn set_view(&mut self, mut view: ViewState) {
        view.size = self.viewport.size();
        self.canvas.set_view(view);
    }

    pub const fn viewport(&self) -> PhysicalRect {
        self.viewport
    }

    pub fn set_viewport(&mut self, viewport: PhysicalRect, background: [u8; 3]) {
        self.viewport = viewport;
        let mut view = self.canvas.view().clone();
        view.size = viewport.size();
        self.canvas.set_view(view);
        self.canvas
            .set_workspace_color([background[0], background[1], background[2], 255]);
        self.sync_canvas_target();
    }

    fn resize_surface(&mut self, size: PhysicalSize) {
        if self.surface_size == size {
            return;
        }
        self.surface_size = size;
        self.sync_canvas_target();
        if size.is_empty() {
            return;
        }
        if self.config.width == size.width && self.config.height == size.height {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn present(&mut self, now: Instant, surface_size: PhysicalSize) -> Result<bool> {
        self.resize_surface(surface_size);
        if self.surface_size.is_empty() {
            return Ok(false);
        }
        let frame = self.canvas.render(now)?;

        let (surface_texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(frame.needs_redraw);
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                bail!("desktop surface returned a validation error")
            }
        };
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("koharu desktop present encoder"),
            });
        self.blitter
            .copy(&self.device, &mut encoder, frame.texture, &surface_view);
        self.queue.submit([encoder.finish()]);
        surface_texture.present();
        if suboptimal {
            self.surface.configure(&self.device, &self.config);
        }
        Ok(frame.needs_redraw)
    }

    fn sync_canvas_target(&mut self) {
        self.canvas.set_render_target(
            self.surface_size,
            PhysicalPoint::new(f64::from(self.viewport.x), f64::from(self.viewport.y)),
        );
    }
}

fn desktop_backends() -> wgpu::Backends {
    wgpu::Backends::PRIMARY | wgpu::Backends::SECONDARY
}

fn adapter_priority(device_type: wgpu::DeviceType, backend: wgpu::Backend) -> (u8, u8) {
    let device = match device_type {
        wgpu::DeviceType::DiscreteGpu => 4,
        wgpu::DeviceType::IntegratedGpu => 3,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 1,
        wgpu::DeviceType::Cpu => 0,
    };
    let backend = match backend {
        wgpu::Backend::Vulkan
        | wgpu::Backend::Metal
        | wgpu::Backend::Dx12
        | wgpu::Backend::BrowserWebGpu => 1,
        wgpu::Backend::Gl | wgpu::Backend::Noop => 0,
    };
    (device, backend)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_viewport_rounds_once_at_the_desktop_boundary() {
        assert_eq!(
            PhysicalRect::from_logical(10.25, 20.5, 300.25, 200.5, 1.5).unwrap(),
            PhysicalRect {
                x: 15,
                y: 31,
                width: 451,
                height: 301,
            }
        );
    }

    #[test]
    fn logical_viewport_rounds_edges_without_a_gap() {
        assert_eq!(
            PhysicalRect::from_logical(0.5, 0.0, 0.5, 1.0, 1.0).unwrap(),
            PhysicalRect {
                x: 1,
                y: 0,
                width: 0,
                height: 1,
            }
        );
    }

    #[test]
    fn invalid_viewports_do_not_reach_wgpu() {
        assert!(PhysicalRect::from_logical(-1.0, 0.0, 10.0, 10.0, 1.0).is_err());
        assert!(PhysicalRect::from_logical(0.0, 0.0, f64::NAN, 10.0, 1.0).is_err());
        assert!(PhysicalRect::from_logical(0.0, 0.0, 10.0, 10.0, 0.0).is_err());
    }
}
