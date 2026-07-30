//! Interactive native playground for `koharu-canvas`.
//!
//! Run with:
//!
//! ```text
//! cargo run -p koharu-canvas --example playground
//! ```
//!
//! This example intentionally talks directly to the public Canvas API. It is a
//! manual integration test for Vello rendering, camera behavior, asynchronous
//! resource loading, and presentation. Editor controls live in React.

use std::{
    io::Cursor,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use koharu_canvas::{
    Camera, Canvas, CanvasGpu, ElementId, Frame, PageId, PhysicalPoint, PhysicalSize, ViewState,
};
use koharu_scene::{
    AssetInput, AssetMetadata, AssetRole, At, Geometry, PageDraft, Point, SceneSession,
};
use vello::wgpu::{self, util::TextureBlitter};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes, WindowId},
};

const TITLE: &str = "Koharu Canvas Playground - wheel zoom | middle-drag pan | F fit | Esc cancel";
const PAGE_WIDTH: u32 = 1_200;
const PAGE_HEIGHT: u32 = 800;

#[derive(Debug)]
enum UserEvent {
    CanvasWake,
    Exit,
}

struct Playground {
    proxy: EventLoopProxy<UserEvent>,
    window: Option<Arc<Window>>,
    state: Option<PlaygroundState>,
    failure: Option<anyhow::Error>,
    auto_exit: bool,
}

impl Playground {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.window.is_some() {
            return Ok(());
        }
        let attributes = WindowAttributes::default()
            .with_title(TITLE)
            .with_inner_size(LogicalSize::new(1_280, 840))
            .with_min_inner_size(LogicalSize::new(640, 480));
        let window = Arc::new(event_loop.create_window(attributes)?);
        let proxy = self.proxy.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = proxy.send_event(UserEvent::CanvasWake);
        });
        let state = pollster::block_on(PlaygroundState::new(Arc::clone(&window), wake))?;
        window.request_redraw();
        self.window = Some(window);
        self.state = Some(state);
        if self.auto_exit {
            let proxy = self.proxy.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(1_200));
                let _ = proxy.send_event(UserEvent::Exit);
            });
        }
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        eprintln!("koharu-canvas playground failed: {error:#}");
        self.failure = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler<UserEvent> for Playground {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.start(event_loop) {
            self.fail(event_loop, error);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::CanvasWake => {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            UserEvent::Exit => event_loop.exit(),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if window.id() != window_id {
            return;
        }
        let Some(state) = self.state.as_mut() else {
            return;
        };

        // The closure gives this infallible Winit callback a small fallible scope,
        // so individual interaction handlers can use `?` and report errors through
        // the same shutdown path.
        let result = (|| -> Result<()> {
            match event {
                WindowEvent::CloseRequested => {
                    event_loop.exit();
                    Ok(())
                }
                WindowEvent::Resized(size) => {
                    state.resize(PhysicalSize::new(size.width, size.height));
                    window.request_redraw();
                    Ok(())
                }
                WindowEvent::ScaleFactorChanged { .. } => {
                    let size = window.inner_size();
                    state.resize(PhysicalSize::new(size.width, size.height));
                    window.request_redraw();
                    Ok(())
                }
                WindowEvent::CursorMoved { position, .. } => {
                    state.pointer_moved(PhysicalPoint::new(position.x, position.y));
                    window.request_redraw();
                    Ok(())
                }
                WindowEvent::MouseInput {
                    state: button_state,
                    button,
                    ..
                } => state.mouse_input(button, button_state).map(|redraw| {
                    if redraw {
                        window.request_redraw();
                    }
                }),
                WindowEvent::MouseWheel { delta, .. } => {
                    state.zoom(delta)?;
                    window.request_redraw();
                    Ok(())
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed && !event.repeat =>
                {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::KeyF) => state.fit_page()?,
                        PhysicalKey::Code(KeyCode::Escape) => state.cancel_interaction(),
                        _ => return Ok(()),
                    }
                    window.request_redraw();
                    Ok(())
                }
                WindowEvent::Focused(false) => {
                    state.cancel_interaction();
                    window.request_redraw();
                    Ok(())
                }
                WindowEvent::Occluded(false) => {
                    window.request_redraw();
                    Ok(())
                }
                WindowEvent::RedrawRequested => state.present(Instant::now()).map(|needs_redraw| {
                    if needs_redraw {
                        window.request_redraw();
                    }
                }),
                _ => Ok(()),
            }
        })();
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

/// Owns the demo scene, the real canvas, and the minimal fullscreen presenter.
struct PlaygroundState {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    config: wgpu::SurfaceConfiguration,
    suspended: bool,
    blitter: TextureBlitter,
    canvas: Canvas,
    _session: SceneSession,
    view: ViewState,
    cursor: PhysicalPoint,
    panning: bool,
}

impl PlaygroundState {
    async fn new(window: Arc<Window>, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .context("no WGPU adapter supports the playground window")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("koharu canvas playground device"),
                ..Default::default()
            })
            .await?;
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let size = window.inner_size();
        let viewport = PhysicalSize::new(size.width, size.height);
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
            .context("the playground surface exposes no texture format")?;
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .context("the playground surface exposes no alpha mode")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: viewport.width.max(1),
            height: viewport.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);
        let blitter = TextureBlitter::new(&device, format);

        let (session, page, _nodes) = demo_scene()?;
        let mut canvas = Canvas::new(
            CanvasGpu {
                device: Arc::clone(&device),
                queue: Arc::clone(&queue),
            },
            wake,
        )?;
        canvas.set_workspace_color([29, 32, 40, 255]);
        let view = ViewState {
            size: viewport,
            camera: Camera::contain(viewport, PhysicalSize::new(PAGE_WIDTH, PAGE_HEIGHT)),
            ..ViewState::default()
        };
        canvas.set_view(view.clone());
        canvas.set_render_target(viewport, PhysicalPoint::default());
        canvas.show_page(&session.snapshot(), page)?;

        println!("{TITLE}");
        println!("Adapter: {}", adapter.get_info().name);
        Ok(Self {
            _instance: instance,
            surface,
            device,
            queue,
            config,
            suspended: viewport.is_empty(),
            blitter,
            canvas,
            _session: session,
            view,
            cursor: PhysicalPoint::default(),
            panning: false,
        })
    }

    fn resize(&mut self, size: PhysicalSize) {
        self.suspended = size.is_empty();
        self.view.size = size;
        self.canvas.set_view(self.view.clone());
        self.canvas
            .set_render_target(size, PhysicalPoint::default());
        if self.suspended {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    fn pointer_moved(&mut self, point: PhysicalPoint) {
        let previous = self.cursor;
        self.cursor = point;
        if self.panning {
            self.view
                .camera
                .pan_by(point.x - previous.x, point.y - previous.y);
            self.canvas.set_view(self.view.clone());
        }
    }

    fn mouse_input(&mut self, button: MouseButton, state: ElementState) -> Result<bool> {
        match (button, state) {
            (MouseButton::Middle, ElementState::Pressed) => {
                self.panning = true;
                Ok(true)
            }
            (MouseButton::Middle, ElementState::Released) => {
                self.panning = false;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn zoom(&mut self, delta: MouseScrollDelta) -> Result<()> {
        let amount = match delta {
            MouseScrollDelta::LineDelta(_, y) => f64::from(y) * 120.0,
            MouseScrollDelta::PixelDelta(position) => position.y,
        };
        let zoom = (self.view.camera.zoom() * (amount * 0.0015).exp()).clamp(0.08, 12.0);
        self.view.camera.zoom_around(self.cursor, zoom)?;
        self.canvas.set_view(self.view.clone());
        Ok(())
    }

    fn fit_page(&mut self) -> Result<()> {
        self.view.camera =
            Camera::contain(self.view.size, PhysicalSize::new(PAGE_WIDTH, PAGE_HEIGHT));
        self.canvas.set_view(self.view.clone());
        Ok(())
    }

    fn cancel_interaction(&mut self) {
        self.panning = false;
    }

    fn present(&mut self, now: Instant) -> Result<bool> {
        if self.suspended {
            return Ok(false);
        }
        let frame = self.canvas.render(now)?;
        let (surface_texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout => return Ok(true),
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(frame.needs_redraw),
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                bail!("the playground surface returned a validation error")
            }
        };
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("koharu canvas playground presenter"),
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
}

fn demo_scene() -> Result<(SceneSession, PageId, Vec<ElementId>)> {
    let mut session = SceneSession::memory()?;
    let mut result = None;
    let nodes = [
        (
            Frame {
                angle_degrees: -7.0,
                ..Frame::new(120.0, 130.0, 280.0, 175.0)
            },
            (560, 350),
            [236, 96, 92],
            [255, 202, 122],
        ),
        (
            Frame {
                angle_degrees: 10.0,
                ..Frame::new(445.0, 275.0, 330.0, 205.0)
            },
            (660, 410),
            [116, 92, 214],
            [205, 176, 255],
        ),
        (
            Frame {
                angle_degrees: -3.0,
                ..Frame::new(820.0, 120.0, 230.0, 285.0)
            },
            (460, 570),
            [38, 160, 145],
            [146, 232, 211],
        ),
    ];
    let patch = session.snapshot().patch(|edit| {
        let page = edit.add_page(
            PageDraft::new(
                "canvas-playground",
                f64::from(PAGE_WIDTH),
                f64::from(PAGE_HEIGHT),
            ),
            At::End,
        )?;
        edit.set_asset(
            page,
            &AssetRole::new("source")?,
            image_asset(page_image(), PAGE_WIDTH, PAGE_HEIGHT),
        )?;
        let mut ids = Vec::new();
        for (frame, size, base, accent) in nodes {
            let id = edit.add_entity(page, At::End)?;
            edit.set(id, "default", &frame_geometry(frame))?;
            edit.set_asset(
                id,
                &AssetRole::new("source")?,
                image_asset(node_image(size, base, accent), size.0, size.1),
            )?;
            ids.push(id);
        }
        result = Some((page, ids));
        Ok(())
    })?;
    session.commit(patch)?;
    let (page, nodes) = result.expect("scene IDs were captured");
    Ok((session, page, nodes))
}

fn image_asset(bytes: Vec<u8>, width: u32, height: u32) -> AssetInput {
    AssetInput::new(
        Arc::<[u8]>::from(bytes),
        "image/png",
        AssetMetadata {
            width: Some(width),
            height: Some(height),
            attributes: Default::default(),
        },
    )
}

fn frame_geometry(frame: Frame) -> Geometry {
    let center_x = f64::from(frame.x + frame.width * 0.5);
    let center_y = f64::from(frame.y + frame.height * 0.5);
    let half_width = f64::from(frame.width) * 0.5;
    let half_height = f64::from(frame.height) * 0.5;
    let angle = f64::from(frame.angle_degrees).to_radians();
    let (sin, cos) = angle.sin_cos();
    Geometry {
        origin: koharu_scene::Origin::User,
        points: [
            (-half_width, -half_height),
            (half_width, -half_height),
            (half_width, half_height),
            (-half_width, half_height),
        ]
        .map(|(x, y)| Point {
            x: center_x + x * cos - y * sin,
            y: center_y + x * sin + y * cos,
        })
        .into(),
    }
}

fn page_image() -> Vec<u8> {
    let image = RgbaImage::from_fn(PAGE_WIDTH, PAGE_HEIGHT, |x, y| {
        let major = x % 200 < 2 || y % 200 < 2;
        let minor = x % 40 == 0 || y % 40 == 0;
        let color = if major {
            [200, 208, 222, 255]
        } else if minor {
            [224, 229, 238, 255]
        } else {
            [245, 247, 251, 255]
        };
        Rgba(color)
    });
    encode_png(image)
}

fn node_image(size: (u32, u32), base: [u8; 3], accent: [u8; 3]) -> Vec<u8> {
    let image = RgbaImage::from_fn(size.0, size.1, |x, y| {
        let border = x < 8 || y < 8 || x >= size.0 - 8 || y >= size.1 - 8;
        let header = y < size.1 / 5;
        let stripe = x > size.0 / 12 && x < size.0 / 12 + 18 && y > size.1 / 3;
        let dot = {
            let dx = i64::from(x) - i64::from(size.0 * 4 / 5);
            let dy = i64::from(y) - i64::from(size.1 * 4 / 5);
            dx * dx + dy * dy < i64::from(size.0.min(size.1) / 7).pow(2)
        };
        let color = if border {
            [35, 40, 52, 255]
        } else if header {
            [base[0], base[1], base[2], 255]
        } else if stripe || dot {
            [accent[0], accent[1], accent[2], 255]
        } else {
            [252, 252, 254, 255]
        };
        Rgba(color)
    });
    encode_png(image)
}

fn encode_png(image: RgbaImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("encoding an in-memory playground image cannot fail");
    bytes.into_inner()
}

fn main() -> Result<()> {
    let auto_exit = std::env::args().any(|argument| argument == "--auto-exit");
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut playground = Playground {
        proxy,
        window: None,
        state: None,
        failure: None,
        auto_exit,
    };
    event_loop.run_app(&mut playground)?;
    if let Some(error) = playground.failure {
        return Err(error);
    }
    Ok(())
}
