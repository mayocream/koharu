use std::{cmp::Reverse, sync::Arc};

use koharu_canvas::{Canvas, CanvasGpu, PhysicalSize, ViewState};
use thiserror::Error;
use vello::wgpu;
use winit::window::Window;

use crate::{
    browser::{BrowserFrame, BrowserGpu},
    compositor::{Composition, Compositor, UiTexture},
    damage::Damage,
    geometry::PhysicalRect,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PresentOutcome {
    pub presented: bool,
    pub needs_redraw: bool,
}

/// Sole owner of the swapchain, WGPU device, canvas, and final
/// compositor. Browser hosts receive no window or surface handle.
pub struct Presenter {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    backend: wgpu::Backend,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    surface_size: PhysicalSize,
    compositor: Compositor,
    canvas: Canvas,
    viewport: PhysicalRect,
    ui: Option<UiTexture>,
    pending_ui: Option<BrowserFrame>,
    damage: Damage,
}

impl Presenter {
    pub async fn new(
        window: Arc<Window>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, PresenterError> {
        let initial = window.inner_size();
        let surface_size = PhysicalSize::new(initial.width, initial.height);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: desktop_backends(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(window)
            .map_err(|error| PresenterError::Surface(error.to_string()))?;
        let (adapter, device, queue) = select_device(&instance, &surface).await?;
        let backend = adapter.get_info().backend;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or(PresenterError::NoSurfaceFormat)?;
        let alpha_mode = capabilities
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
            .ok_or(PresenterError::NoAlphaMode)?;
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::AutoVsync)
            .or_else(|| capabilities.present_modes.first().copied())
            .ok_or(PresenterError::NoPresentMode)?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: surface_size.width.max(1),
            height: surface_size.height.max(1),
            present_mode,
            color_space: wgpu::SurfaceColorSpace::Auto,
            desired_maximum_frame_latency: 1,
            alpha_mode,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);
        let mut canvas = Canvas::new(
            CanvasGpu {
                device: Arc::clone(&device),
                queue: Arc::clone(&queue),
            },
            wake,
        )
        .map_err(|error| PresenterError::Canvas(error.to_string()))?;
        canvas.set_view(ViewState::default());
        let compositor = Compositor::new(&device, format);
        Ok(Self {
            device,
            queue,
            backend,
            surface,
            config,
            surface_size,
            compositor,
            canvas,
            viewport: PhysicalRect::default(),
            ui: None,
            pending_ui: None,
            damage: Damage::initial(),
        })
    }

    #[must_use]
    pub const fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    pub fn canvas_mut(&mut self) -> &mut Canvas {
        self.damage.canvas();
        &mut self.canvas
    }

    #[must_use]
    pub const fn viewport(&self) -> PhysicalRect {
        self.viewport
    }

    #[must_use]
    pub const fn surface_size(&self) -> PhysicalSize {
        self.surface_size
    }

    pub(crate) fn browser_gpu(&self) -> BrowserGpu {
        BrowserGpu {
            device: Arc::clone(&self.device),
            queue: Arc::clone(&self.queue),
            backend: self.backend,
        }
    }

    pub fn set_viewport(&mut self, viewport: PhysicalRect, workspace: [u8; 3]) {
        let viewport = viewport.clipped_to(self.surface_size);
        if self.viewport != viewport {
            self.viewport = viewport;
            let mut view = self.canvas.view().clone();
            view.size = viewport.size();
            self.canvas.set_view(view);
            self.damage.canvas();
        }
        self.canvas
            .set_workspace_color([workspace[0], workspace[1], workspace[2], 255]);
    }

    pub fn set_canvas_view(&mut self, mut view: ViewState) {
        view.size = self.viewport.size();
        self.canvas.set_view(view);
        self.damage.canvas();
    }

    pub fn resize(&mut self, size: PhysicalSize) {
        if self.surface_size == size {
            return;
        }
        self.surface_size = size;
        self.viewport = self.viewport.clipped_to(size);
        let mut view = self.canvas.view().clone();
        view.size = self.viewport.size();
        self.canvas.set_view(view);
        if !size.is_empty() {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
        }
        self.damage.surface();
    }

    pub(crate) fn offer_ui_frame(&mut self, frame: BrowserFrame) {
        self.pending_ui = Some(frame);
        self.damage.ui();
    }

    #[must_use]
    pub fn needs_redraw(&self) -> bool {
        self.damage.pending() || self.canvas.needs_redraw()
    }

    pub fn present(&mut self) -> Result<PresentOutcome, PresenterError> {
        if let Some(frame) = self.pending_ui.take() {
            if frame.width() == self.surface_size.width
                && frame.height() == self.surface_size.height
            {
                self.ui = Some(UiTexture::install(
                    self.ui.take(),
                    &self.device,
                    &self.queue,
                    frame,
                ));
            } else {
                tracing::debug!(
                    frame_width = frame.width(),
                    frame_height = frame.height(),
                    surface_width = self.surface_size.width,
                    surface_height = self.surface_size.height,
                    "discarded stale browser frame after window resize"
                );
            }
        }

        if self.surface_size.is_empty() || !self.needs_redraw() {
            return Ok(PresentOutcome {
                needs_redraw: self.needs_redraw(),
                ..PresentOutcome::default()
            });
        }

        let canvas = self
            .canvas
            .render()
            .map_err(|error| PresenterError::Canvas(error.to_string()))?;
        let (surface_texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(PresentOutcome {
                    needs_redraw: true,
                    ..PresentOutcome::default()
                });
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                self.damage.surface();
                return Ok(PresentOutcome {
                    needs_redraw: true,
                    ..PresentOutcome::default()
                });
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(PresenterError::SurfaceValidation);
            }
        };
        let target = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("koharu sole desktop present encoder"),
            });
        self.compositor.compose(
            &self.device,
            &mut encoder,
            Composition {
                target: &target,
                surface_size: self.surface_size,
                viewport: self.viewport,
                canvas: canvas.texture,
                ui: self.ui.as_ref().map(UiTexture::view),
            },
        );
        self.queue.submit([encoder.finish()]);
        self.queue.present(surface_texture);
        self.damage.clear_presented();
        if canvas.needs_redraw {
            self.damage.canvas();
        }
        if suboptimal {
            self.surface.configure(&self.device, &self.config);
            self.damage.surface();
        }
        Ok(PresentOutcome {
            presented: true,
            needs_redraw: self.needs_redraw(),
        })
    }
}

async fn select_device(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'_>,
) -> Result<(wgpu::Adapter, Arc<wgpu::Device>, Arc<wgpu::Queue>), PresenterError> {
    let mut adapters = instance
        .enumerate_adapters(desktop_backends())
        .await
        .into_iter()
        .filter(|adapter| adapter.is_surface_supported(surface))
        .map(|adapter| (adapter.get_info(), adapter))
        .collect::<Vec<_>>();
    adapters.sort_by_key(|(info, _)| Reverse(adapter_priority(info.device_type, info.backend)));

    let mut failures = Vec::new();
    for (info, adapter) in adapters {
        match adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("koharu desktop device"),
                required_features: wgpu::Features::IMMEDIATES,
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
        {
            Ok((device, queue)) => {
                tracing::info!(adapter = ?info, "created sole desktop WGPU context");
                return Ok((adapter, Arc::new(device), Arc::new(queue)));
            }
            Err(error) => failures.push(format!("{} ({}): {error}", info.name, info.backend)),
        }
    }
    Err(PresenterError::NoAdapter(failures.join("; ")))
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

#[derive(Debug, Error)]
pub enum PresenterError {
    #[error("failed to create WGPU surface: {0}")]
    Surface(String),
    #[error("no WGPU adapter supports the surface: {0}")]
    NoAdapter(String),
    #[error("surface exposes no texture format")]
    NoSurfaceFormat,
    #[error("surface exposes no alpha mode")]
    NoAlphaMode,
    #[error("surface exposes no present mode")]
    NoPresentMode,
    #[error("canvas failed: {0}")]
    Canvas(String),
    #[error("surface returned a validation error")]
    SurfaceValidation,
}
