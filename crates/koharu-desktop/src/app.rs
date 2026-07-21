use std::{
    borrow::Cow,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use anyhow::{Context as _, Result, anyhow};
use koharu_canvas::{Canvas, MaskCommit, OverlayState, ViewState};
use koharu_scene::{ChangeSet, PageId, Session};
use serde::Serialize;
use serde_json::Value;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{ResizeDirection, Window, WindowAttributes, WindowId},
};
use wry::{
    Rect, RequestAsyncResponder, WebView, WebViewBuilder,
    http::{Response, header::CONTENT_TYPE},
};

use crate::{
    Gpu, MaskEncodingResult, PhysicalRect,
    gpu::Renderer,
    mask::MaskEncoder,
    protocol::{BRIDGE_SCRIPT, IncomingMessage, WindowAction, decode_message},
};

const EMPTY_FRONTEND: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><style>
html,body { width:100%; height:100%; margin:0; background:transparent; }
</style></head><body></body></html>"#;
const MAX_IPC_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub enum Frontend {
    Html(String),
    Url(String),
    /// Serve a static web application without a local HTTP server.
    Directory(PathBuf),
    /// Serve a static web application from assets owned by the executable.
    Embedded(Arc<dyn Fn(&str) -> Option<Cow<'static, [u8]>> + Send + Sync + 'static>),
}

impl Frontend {
    #[must_use]
    pub fn embedded(
        get: impl Fn(&str) -> Option<Cow<'static, [u8]>> + Send + Sync + 'static,
    ) -> Self {
        Self::Embedded(Arc::new(get))
    }
}

impl std::fmt::Debug for Frontend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Html(_) => formatter.write_str("Html(..)"),
            Self::Url(url) => formatter.debug_tuple("Url").field(url).finish(),
            Self::Directory(root) => formatter.debug_tuple("Directory").field(root).finish(),
            Self::Embedded(_) => formatter.write_str("Embedded(..)"),
        }
    }
}

impl Default for Frontend {
    fn default() -> Self {
        Self::Html(EMPTY_FRONTEND.into())
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub min_width: u32,
    pub min_height: u32,
    pub resizable: bool,
    pub decorations: bool,
    pub devtools: bool,
    pub frontend: Frontend,
    pub protocols: Vec<CustomProtocol>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            title: "Koharu".into(),
            width: 1280,
            height: 800,
            min_width: 800,
            min_height: 520,
            resizable: true,
            decorations: true,
            devtools: cfg!(debug_assertions),
            frontend: Frontend::default(),
            protocols: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProtocolRequest {
    pub method: String,
    pub uri: String,
}

#[derive(Clone, Debug)]
pub struct ProtocolResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl ProtocolResponse {
    #[must_use]
    pub fn new(status: u16, content_type: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body,
            headers: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

pub struct ProtocolResponder(RequestAsyncResponder);

impl ProtocolResponder {
    pub fn respond(self, response: ProtocolResponse) {
        let mut builder = Response::builder()
            .status(response.status)
            .header(CONTENT_TYPE, response.content_type);
        for (name, value) in response.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .body(response.body)
            .expect("custom protocol response headers are valid");
        self.0.respond(response);
    }
}

type ProtocolHandler = dyn Fn(ProtocolRequest, ProtocolResponder) + Send + Sync + 'static;

#[derive(Clone)]
pub struct CustomProtocol {
    scheme: String,
    handler: Arc<ProtocolHandler>,
}

impl CustomProtocol {
    #[must_use]
    pub fn new(
        scheme: impl Into<String>,
        handler: impl Fn(ProtocolRequest, ProtocolResponder) + Send + Sync + 'static,
    ) -> Self {
        Self {
            scheme: scheme.into(),
            handler: Arc::new(handler),
        }
    }
}

impl std::fmt::Debug for CustomProtocol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CustomProtocol")
            .field("scheme", &self.scheme)
            .finish_non_exhaustive()
    }
}

pub trait Application: 'static {
    type Event: Send + 'static;

    fn started(&mut self, _desktop: &mut DesktopContext<'_, Self::Event>) -> Result<()> {
        Ok(())
    }

    fn ready(
        &mut self,
        _desktop: &mut DesktopContext<'_, Self::Event>,
        _dpr: f64,
        _width: f64,
        _height: f64,
    ) -> Result<()> {
        Ok(())
    }

    fn message(
        &mut self,
        _desktop: &mut DesktopContext<'_, Self::Event>,
        _message: Value,
    ) -> Result<()> {
        Ok(())
    }

    fn event(
        &mut self,
        _desktop: &mut DesktopContext<'_, Self::Event>,
        _event: Self::Event,
    ) -> Result<()> {
        Ok(())
    }

    fn viewport_changed(&mut self, _desktop: &mut DesktopContext<'_, Self::Event>) -> Result<()> {
        Ok(())
    }

    fn mask_encoded(
        &mut self,
        _desktop: &mut DesktopContext<'_, Self::Event>,
        _result: MaskEncodingResult,
    ) -> Result<()> {
        Ok(())
    }

    fn close_requested(&mut self, _desktop: &mut DesktopContext<'_, Self::Event>) -> Result<bool> {
        Ok(true)
    }
}

pub struct DesktopHandle<E: Send + 'static> {
    proxy: EventLoopProxy<UserEvent<E>>,
    redraw_event_pending: Arc<AtomicBool>,
}

impl<E: Send + 'static> Clone for DesktopHandle<E> {
    fn clone(&self) -> Self {
        Self {
            proxy: self.proxy.clone(),
            redraw_event_pending: self.redraw_event_pending.clone(),
        }
    }
}

impl<E: Send + 'static> DesktopHandle<E> {
    pub fn send_event(&self, event: E) -> Result<()> {
        self.send(UserEvent::Application(event))
    }

    pub fn request_redraw(&self) -> Result<()> {
        if self.redraw_event_pending.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        if let Err(error) = self.send(UserEvent::RequestRedraw) {
            self.redraw_event_pending.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(())
    }

    pub fn emit<T: Serialize>(&self, name: impl Into<String>, payload: T) -> Result<()> {
        self.send(UserEvent::Emit {
            name: name.into(),
            payload: serde_json::to_value(payload)?,
        })
    }

    pub fn exit(&self) -> Result<()> {
        self.send(UserEvent::Exit)
    }

    fn send(&self, event: UserEvent<E>) -> Result<()> {
        self.proxy
            .send_event(event)
            .map_err(|_| anyhow!("desktop event loop is no longer running"))
    }
}

pub struct DesktopContext<'a, E: Send + 'static> {
    handle: DesktopHandle<E>,
    window: &'a Arc<Window>,
    webview: &'a WebView,
    renderer: &'a mut Renderer,
    masks: &'a mut MaskEncoder,
    redraw_requested: &'a mut bool,
}

impl<E: Send + 'static> DesktopContext<'_, E> {
    #[must_use]
    pub fn handle(&self) -> DesktopHandle<E> {
        self.handle.clone()
    }

    #[must_use]
    pub fn gpu(&self) -> &Arc<Gpu> {
        self.renderer.gpu()
    }

    #[must_use]
    pub fn viewport(&self) -> PhysicalRect {
        self.renderer.viewport()
    }

    #[must_use]
    pub fn view(&self) -> &ViewState {
        self.renderer.view()
    }

    pub fn set_view(&mut self, view: ViewState) {
        self.renderer.set_view(view);
        self.request_redraw();
    }

    pub fn set_overlays(&mut self, overlays: OverlayState) {
        self.renderer.canvas().set_overlays(overlays);
        self.request_redraw();
    }

    pub fn show_page(&mut self, session: &Session, page: PageId) -> koharu_canvas::Result<()> {
        self.renderer.canvas().show_page(session, page)?;
        self.request_redraw();
        Ok(())
    }

    pub fn clear_page(&mut self) {
        self.renderer.canvas().clear_page();
        self.request_redraw();
    }

    pub fn sync(&mut self, session: &Session, changes: &ChangeSet) -> koharu_canvas::Result<()> {
        self.renderer.canvas().sync(session, changes)?;
        self.request_redraw();
        Ok(())
    }

    /// Access advanced canvas operations. Merely borrowing the canvas schedules a frame,
    /// so callers cannot forget to present mutations made through the returned value.
    pub fn canvas(&mut self) -> &mut Canvas {
        self.request_redraw();
        self.renderer.canvas()
    }

    pub fn submit_mask(&mut self, commit: MaskCommit) {
        self.masks.submit(commit);
    }

    pub fn emit<T: Serialize>(&self, name: &str, payload: T) -> Result<()> {
        emit(self.webview, name, serde_json::to_value(payload)?)
    }

    pub fn evaluate_script(&self, script: &str) -> Result<()> {
        self.webview.evaluate_script(script)?;
        Ok(())
    }

    pub fn request_redraw(&mut self) {
        request_redraw(self.window, self.redraw_requested);
    }
}

enum UserEvent<E> {
    Ipc(String),
    Application(E),
    WorkReady,
    Emit { name: String, payload: Value },
    RequestRedraw,
    Exit,
}

struct Shell<A: Application> {
    options: Options,
    application: A,
    proxy: EventLoopProxy<UserEvent<A::Event>>,
    window: Option<Arc<Window>>,
    webview: Option<WebView>,
    renderer: Option<Renderer>,
    masks: Option<MaskEncoder>,
    redraw_event_pending: Arc<AtomicBool>,
    work_event_pending: Arc<AtomicBool>,
    redraw_requested: bool,
    failure: Option<anyhow::Error>,
}

impl<A: Application> Shell<A> {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.window.is_some() {
            return Ok(());
        }

        let mut attributes = WindowAttributes::default()
            .with_title(&self.options.title)
            .with_inner_size(LogicalSize::new(self.options.width, self.options.height))
            .with_min_inner_size(LogicalSize::new(
                self.options.min_width,
                self.options.min_height,
            ))
            .with_resizable(self.options.resizable)
            .with_decorations(self.options.decorations)
            .with_transparent(true)
            .with_visible(false);
        #[cfg(windows)]
        {
            use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows as _};
            attributes = attributes
                .with_clip_children(false)
                .with_undecorated_shadow(true)
                .with_corner_preference(CornerPreference::Round);
        }

        let window = Arc::new(event_loop.create_window(attributes)?);
        let proxy = self.proxy.clone();
        let pending = Arc::clone(&self.work_event_pending);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if !pending.swap(true, Ordering::AcqRel)
                && proxy.send_event(UserEvent::WorkReady).is_err()
            {
                pending.store(false, Ordering::Release);
            }
        });
        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window), Arc::clone(&wake)))?;
        let masks = MaskEncoder::new(wake);

        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        let ipc = self.proxy.clone();
        let mut builder = WebViewBuilder::new()
            .with_bounds(Rect {
                position: wry::dpi::LogicalPosition::new(0, 0).into(),
                size: wry::dpi::LogicalSize::new(size.width, size.height).into(),
            })
            .with_transparent(true)
            .with_devtools(self.options.devtools)
            .with_initialization_script(BRIDGE_SCRIPT)
            .with_ipc_handler(move |request| {
                let _ = ipc.send_event(UserEvent::Ipc(request.body().clone()));
            });
        for protocol in &self.options.protocols {
            let handler = Arc::clone(&protocol.handler);
            builder = builder.with_asynchronous_custom_protocol(
                protocol.scheme.clone(),
                move |_webview_id, request, responder| {
                    handler(
                        ProtocolRequest {
                            method: request.method().to_string(),
                            uri: request.uri().to_string(),
                        },
                        ProtocolResponder(responder),
                    );
                },
            );
        }
        let builder = match &self.options.frontend {
            Frontend::Html(html) => builder.with_html(html),
            Frontend::Url(url) => builder.with_url(url),
            Frontend::Directory(root) => {
                let root = root.canonicalize().with_context(|| {
                    format!("frontend directory {} does not exist", root.display())
                })?;
                builder
                    .with_custom_protocol("koharu".into(), move |_id, request| {
                        static_response(&root, request.uri().path())
                    })
                    .with_url("koharu://localhost/")
            }
            Frontend::Embedded(get) => {
                let get = Arc::clone(get);
                builder
                    .with_custom_protocol("koharu".into(), move |_id, request| {
                        embedded_response(get.as_ref(), request.uri().path())
                    })
                    .with_url("koharu://localhost/")
            }
        };
        let webview = builder
            .build_as_child(&window)
            .context("failed to create the Koharu UI webview")?;

        self.window = Some(window);
        self.webview = Some(webview);
        self.renderer = Some(renderer);
        self.masks = Some(masks);
        self.call_application(|application, desktop| application.started(desktop))?;

        let window = self.window.as_ref().expect("window installed above");
        window.set_visible(true);
        window.focus_window();
        self.request_redraw();
        Ok(())
    }

    fn call_application(
        &mut self,
        call: impl FnOnce(&mut A, &mut DesktopContext<'_, A::Event>) -> Result<()>,
    ) -> Result<()> {
        let window = self
            .window
            .as_ref()
            .context("desktop window is unavailable")?;
        let webview = self
            .webview
            .as_ref()
            .context("desktop webview is unavailable")?;
        let renderer = self
            .renderer
            .as_mut()
            .context("desktop renderer is unavailable")?;
        let masks = self
            .masks
            .as_mut()
            .context("desktop mask encoder is unavailable")?;
        let mut desktop = DesktopContext {
            handle: DesktopHandle {
                proxy: self.proxy.clone(),
                redraw_event_pending: Arc::clone(&self.redraw_event_pending),
            },
            window,
            webview,
            renderer,
            masks,
            redraw_requested: &mut self.redraw_requested,
        };
        call(&mut self.application, &mut desktop)
    }

    fn close_requested(&mut self) -> Result<bool> {
        let mut close = false;
        self.call_application(|application, desktop| {
            close = application.close_requested(desktop)?;
            Ok(())
        })?;
        Ok(close)
    }

    fn handle_ipc(&mut self, event_loop: &ActiveEventLoop, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_IPC_BYTES {
            tracing::warn!(bytes = bytes.len(), "ignored oversized desktop message");
            return Ok(());
        }
        let message = match decode_message(bytes) {
            Ok(message) => message,
            Err(error) => {
                tracing::warn!(%error, "ignored invalid desktop message");
                return Ok(());
            }
        };
        match message {
            IncomingMessage::Ready { dpr, width, height } => {
                if ![dpr, width, height].into_iter().all(f64::is_finite)
                    || dpr <= 0.0
                    || width < 0.0
                    || height < 0.0
                {
                    tracing::warn!("ignored invalid desktop ready message");
                    return Ok(());
                }
                self.call_application(|application, desktop| {
                    application.ready(desktop, dpr, width, height)
                })
            }
            IncomingMessage::Viewport {
                x,
                y,
                width,
                height,
                dpr,
                background,
            } => {
                let viewport = match PhysicalRect::from_logical(x, y, width, height, dpr) {
                    Ok(viewport) => viewport,
                    Err(error) => {
                        tracing::warn!(%error, "ignored invalid desktop viewport");
                        return Ok(());
                    }
                };
                let renderer = self
                    .renderer
                    .as_mut()
                    .context("desktop renderer is unavailable")?;
                renderer.set_viewport(viewport);
                renderer.set_background(background);
                self.request_redraw();
                self.call_application(|application, desktop| application.viewport_changed(desktop))
            }
            IncomingMessage::Window(action) => self.window_action(event_loop, action),
            IncomingMessage::Application(message) => {
                self.call_application(|application, desktop| application.message(desktop, message))
            }
        }
    }

    fn window_action(&mut self, event_loop: &ActiveEventLoop, action: WindowAction) -> Result<()> {
        let operation = window_operation(action);
        if matches!(operation, WindowOperation::Close) {
            return self.close_requested().map(|close| {
                if close {
                    event_loop.exit();
                }
            });
        }
        let window = self
            .window
            .as_ref()
            .context("desktop window is unavailable")?;
        match operation {
            WindowOperation::Drag => window
                .drag_window()
                .context("failed to drag desktop window"),
            WindowOperation::Resize(direction) => window
                .drag_resize_window(direction)
                .context("failed to resize desktop window"),
            WindowOperation::Minimize => {
                window.set_minimized(true);
                Ok(())
            }
            WindowOperation::ToggleMaximize => {
                window.set_maximized(!window.is_maximized());
                Ok(())
            }
            WindowOperation::Close => {
                unreachable!("close actions return before borrowing the window")
            }
        }
    }

    fn work_ready(&mut self) -> Result<()> {
        self.work_event_pending.store(false, Ordering::Release);
        let completed = self
            .masks
            .as_mut()
            .context("desktop mask encoder is unavailable")?
            .drain();
        for result in completed {
            self.call_application(|application, desktop| {
                application.mask_encoded(desktop, result)
            })?;
        }
        self.request_redraw();
        Ok(())
    }

    fn resize_webview(&self) -> Result<()> {
        let window = self
            .window
            .as_ref()
            .context("desktop window is unavailable")?;
        let webview = self
            .webview
            .as_ref()
            .context("desktop webview is unavailable")?;
        let logical = window.inner_size().to_logical::<f64>(window.scale_factor());
        webview.set_bounds(Rect {
            position: wry::dpi::LogicalPosition::new(0, 0).into(),
            size: wry::dpi::LogicalSize::new(logical.width, logical.height).into(),
        })?;
        Ok(())
    }

    fn request_redraw(&mut self) {
        if let Some(window) = self.window.as_ref() {
            request_redraw(window, &mut self.redraw_requested);
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        tracing::error!(error = ?error, "desktop application failed");
        self.failure = Some(error);
        event_loop.exit();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowOperation {
    Drag,
    Resize(ResizeDirection),
    Minimize,
    ToggleMaximize,
    Close,
}

const fn window_operation(action: WindowAction) -> WindowOperation {
    match action {
        WindowAction::Drag => WindowOperation::Drag,
        WindowAction::ResizeEast => WindowOperation::Resize(ResizeDirection::East),
        WindowAction::ResizeNorth => WindowOperation::Resize(ResizeDirection::North),
        WindowAction::ResizeNorthEast => WindowOperation::Resize(ResizeDirection::NorthEast),
        WindowAction::ResizeNorthWest => WindowOperation::Resize(ResizeDirection::NorthWest),
        WindowAction::ResizeSouth => WindowOperation::Resize(ResizeDirection::South),
        WindowAction::ResizeSouthEast => WindowOperation::Resize(ResizeDirection::SouthEast),
        WindowAction::ResizeSouthWest => WindowOperation::Resize(ResizeDirection::SouthWest),
        WindowAction::ResizeWest => WindowOperation::Resize(ResizeDirection::West),
        WindowAction::Minimize => WindowOperation::Minimize,
        WindowAction::ToggleMaximize => WindowOperation::ToggleMaximize,
        WindowAction::Close => WindowOperation::Close,
    }
}

impl<A: Application> ApplicationHandler<UserEvent<A::Event>> for Shell<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.start(event_loop) {
            self.fail(event_loop, error);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent<A::Event>) {
        let result = match event {
            UserEvent::Ipc(message) => self.handle_ipc(event_loop, message.as_bytes()),
            UserEvent::Application(event) => {
                self.call_application(|application, desktop| application.event(desktop, event))
            }
            UserEvent::WorkReady => self.work_ready(),
            UserEvent::Emit { name, payload } => self
                .webview
                .as_ref()
                .context("desktop webview is unavailable")
                .and_then(|webview| emit(webview, &name, payload)),
            UserEvent::RequestRedraw => {
                self.redraw_event_pending.store(false, Ordering::Release);
                self.request_redraw();
                Ok(())
            }
            UserEvent::Exit => {
                event_loop.exit();
                Ok(())
            }
        };
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        let result = match event {
            WindowEvent::CloseRequested => self.close_requested().map(|close| {
                if close {
                    event_loop.exit();
                }
            }),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer
                        .resize_surface(koharu_canvas::PhysicalSize::new(size.width, size.height));
                }
                self.resize_webview().map(|()| self.request_redraw())
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.resize_webview().map(|()| self.request_redraw())
            }
            WindowEvent::Occluded(false) => {
                self.request_redraw();
                Ok(())
            }
            WindowEvent::RedrawRequested => {
                self.redraw_requested = false;
                self.renderer
                    .as_mut()
                    .context("desktop renderer is unavailable")
                    .and_then(|renderer| renderer.present(Instant::now()))
                    .map(|presented| {
                        if presented.needs_redraw {
                            self.request_redraw();
                        }
                    })
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

pub fn run<A: Application>(options: Options, application: A) -> Result<()> {
    let event_loop = EventLoop::<UserEvent<A::Event>>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut shell = Shell {
        options,
        application,
        proxy,
        window: None,
        webview: None,
        renderer: None,
        masks: None,
        redraw_event_pending: Arc::new(AtomicBool::new(false)),
        work_event_pending: Arc::new(AtomicBool::new(false)),
        redraw_requested: false,
        failure: None,
    };
    event_loop.run_app(&mut shell)?;
    if let Some(error) = shell.failure {
        return Err(error);
    }
    Ok(())
}

fn request_redraw(window: &Window, requested: &mut bool) {
    if !*requested {
        window.request_redraw();
        *requested = true;
    }
}

fn emit(webview: &WebView, name: &str, payload: Value) -> Result<()> {
    let detail = serde_json::json!({ "type": name, "payload": payload });
    let detail = serde_json::to_string(&detail)?;
    webview.evaluate_script(&format!(
        "window.dispatchEvent(new CustomEvent('koharu:event', {{detail:{detail}}}));"
    ))?;
    Ok(())
}

fn static_response(root: &Path, request_path: &str) -> Response<Cow<'static, [u8]>> {
    let Ok(relative) = frontend_path(request_path) else {
        return response(400, "text/plain; charset=utf-8", b"invalid path".to_vec());
    };
    let candidate = match root.join(&relative).canonicalize() {
        Ok(candidate) if candidate.starts_with(root) => candidate,
        _ => return response(404, "text/plain; charset=utf-8", b"not found".to_vec()),
    };
    match std::fs::read(&candidate) {
        Ok(bytes) => response(200, mime_type(&candidate), bytes),
        Err(_) => response(404, "text/plain; charset=utf-8", b"not found".to_vec()),
    }
}

fn embedded_response(
    get: &(dyn Fn(&str) -> Option<Cow<'static, [u8]>> + Send + Sync),
    request_path: &str,
) -> Response<Cow<'static, [u8]>> {
    let relative = match frontend_path(request_path) {
        Ok(relative) => relative,
        Err(()) => return response(400, "text/plain; charset=utf-8", b"invalid path".to_vec()),
    };
    match get(&relative) {
        Some(bytes) => response(200, mime_type(Path::new(&relative)), bytes),
        None => response(404, "text/plain; charset=utf-8", b"not found".to_vec()),
    }
}

fn frontend_path(request_path: &str) -> Result<String, ()> {
    let relative = request_path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    if Path::new(relative)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(());
    }
    Ok(relative.replace('\\', "/"))
}

fn response(
    status: u16,
    content_type: &'static str,
    bytes: impl Into<Cow<'static, [u8]>>,
) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, content_type)
        .body(bytes.into())
        .expect("static response has valid headers")
}

fn mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_actions_map_to_platform_operations_without_a_window() {
        assert_eq!(window_operation(WindowAction::Drag), WindowOperation::Drag);
        assert_eq!(
            window_operation(WindowAction::ResizeEast),
            WindowOperation::Resize(ResizeDirection::East)
        );
        assert_eq!(
            window_operation(WindowAction::ResizeNorth),
            WindowOperation::Resize(ResizeDirection::North)
        );
        assert_eq!(
            window_operation(WindowAction::ResizeNorthEast),
            WindowOperation::Resize(ResizeDirection::NorthEast)
        );
        assert_eq!(
            window_operation(WindowAction::ResizeNorthWest),
            WindowOperation::Resize(ResizeDirection::NorthWest)
        );
        assert_eq!(
            window_operation(WindowAction::ResizeSouth),
            WindowOperation::Resize(ResizeDirection::South)
        );
        assert_eq!(
            window_operation(WindowAction::ResizeSouthEast),
            WindowOperation::Resize(ResizeDirection::SouthEast)
        );
        assert_eq!(
            window_operation(WindowAction::ResizeSouthWest),
            WindowOperation::Resize(ResizeDirection::SouthWest)
        );
        assert_eq!(
            window_operation(WindowAction::ResizeWest),
            WindowOperation::Resize(ResizeDirection::West)
        );
        assert_eq!(
            window_operation(WindowAction::Minimize),
            WindowOperation::Minimize
        );
        assert_eq!(
            window_operation(WindowAction::ToggleMaximize),
            WindowOperation::ToggleMaximize
        );
        assert_eq!(
            window_operation(WindowAction::Close),
            WindowOperation::Close
        );
    }

    #[test]
    fn static_frontend_is_rooted_and_typed() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("index.html"), "<h1>Koharu</h1>").unwrap();
        let root = root.path().canonicalize().unwrap();

        let response = static_response(&root, "/");
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(response.body().as_ref(), b"<h1>Koharu</h1>");

        assert_eq!(static_response(&root, "/../secret").status(), 400);
        assert_eq!(static_response(&root, "/missing.js").status(), 404);
    }

    #[test]
    fn embedded_frontend_is_rooted_and_typed() {
        let get = |path: &str| {
            (path == "index.html").then(|| Cow::Borrowed(b"<h1>Koharu</h1>".as_slice()))
        };

        let response = embedded_response(&get, "/");
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(response.body().as_ref(), b"<h1>Koharu</h1>");

        assert_eq!(embedded_response(&get, "/../secret").status(), 400);
        assert_eq!(embedded_response(&get, "/missing.js").status(), 404);
    }
}
