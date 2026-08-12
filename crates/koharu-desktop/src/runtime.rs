use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use koharu_canvas::PhysicalSize;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, Ime, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, KeyLocation as WinitKeyLocation, ModifiersState, NamedKey, PhysicalKey},
    platform::{
        modifier_supplement::KeyEventExtModifierSupplement, scancode::PhysicalKeyExtScancode,
    },
    window::{Icon, Window, WindowId},
};

use crate::{
    Presenter,
    browser::{
        BrowserCursor, ButtonState, CefBrowser, CefConfig, CefEvent, CefEventSender, InputEvent,
        InputModifiers, KeyLocation, KeyState, MouseButton, ScrollDelta, WebMessage,
    },
    geometry::{Layout, LogicalRect},
    platform::{PlatformError, PlatformServices, WindowAction, WindowActionError, WindowState},
};

const MINIMUM_WINDOW_WIDTH: f64 = 900.;
const MINIMUM_WINDOW_HEIGHT: f64 = 600.;

#[derive(Clone, Debug)]
pub struct DesktopConfig {
    pub title: Box<str>,
    pub icon_png: &'static [u8],
    pub initial_width: f64,
    pub initial_height: f64,
    pub cef_distribution: PathBuf,
    pub browser_cache_root: PathBuf,
    pub initial_url: Box<str>,
    pub browser_resources_root: Option<PathBuf>,
}

#[derive(Debug)]
pub enum DesktopEvent {
    Browser(CefEvent),
    SendWebMessage(WebMessage),
    Viewport {
        id: u64,
        bounds: LogicalRect,
        scale_factor: f64,
        workspace_color: [u8; 3],
    },
    Window {
        id: u64,
        action: WindowAction,
    },
    OpenExternal {
        id: u64,
        url: url::Url,
    },
    Wake,
    Shutdown,
}

#[derive(Clone)]
pub struct DesktopHandle {
    proxy: EventLoopProxy<DesktopEvent>,
}

impl DesktopHandle {
    pub fn send_web_message(&self, message: WebMessage) -> Result<(), DesktopClosed> {
        self.send(DesktopEvent::SendWebMessage(message))
    }

    /// Reports browser-owned logical canvas bounds. The desktop runtime binds
    /// the report to its current window generation so callers never fabricate
    /// window resize sequencing state.
    pub fn set_viewport(
        &self,
        id: u64,
        bounds: LogicalRect,
        scale_factor: f64,
        workspace_color: [u8; 3],
    ) -> Result<(), DesktopClosed> {
        self.send(DesktopEvent::Viewport {
            id,
            bounds,
            scale_factor,
            workspace_color,
        })
    }

    pub fn window(&self, id: u64, action: WindowAction) -> Result<(), DesktopClosed> {
        self.send(DesktopEvent::Window { id, action })
    }

    pub fn open_external(&self, id: u64, url: url::Url) -> Result<(), DesktopClosed> {
        self.send(DesktopEvent::OpenExternal { id, url })
    }

    pub fn wake(&self) -> Result<(), DesktopClosed> {
        self.send(DesktopEvent::Wake)
    }

    pub fn shutdown(&self) -> Result<(), DesktopClosed> {
        self.send(DesktopEvent::Shutdown)
    }

    fn send(&self, event: DesktopEvent) -> Result<(), DesktopClosed> {
        self.proxy.send_event(event).map_err(|_| DesktopClosed)
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("desktop event loop is closed")]
pub struct DesktopClosed;

/// Application-facing callbacks. Implementations may keep their own
/// thread-safe command queue and drain it from `wake`, mutating the canvas only
/// on the winit thread through the provided presenter.
pub trait DesktopDelegate: 'static {
    fn browser_ready(&mut self, _handle: &DesktopHandle) {}

    fn browser_message(
        &mut self,
        message: WebMessage,
        presenter: &mut Presenter,
        handle: &DesktopHandle,
    );

    fn wake(&mut self, _presenter: &mut Presenter, _handle: &DesktopHandle) {}

    fn platform_response(
        &mut self,
        _id: u64,
        _response: Result<(), PlatformError>,
        _handle: &DesktopHandle,
    ) {
    }

    fn window_action_response(
        &mut self,
        _id: u64,
        _action: WindowAction,
        _response: Result<WindowState, WindowActionError>,
        _handle: &DesktopHandle,
    ) {
    }

    fn window_state_changed(&mut self, _state: WindowState, _handle: &DesktopHandle) {}

    /// Called on the winit thread after DPR/generation validation and,
    /// on success, after the presenter's physical viewport has been updated.
    /// The delegate may refit the canvas and return authoritative state without
    /// duplicating desktop rounding or generation logic.
    fn viewport_applied(
        &mut self,
        _id: u64,
        _result: Result<crate::geometry::PhysicalRect, crate::geometry::LayoutError>,
        _presenter: &mut Presenter,
        _handle: &DesktopHandle,
    ) {
    }

    fn fatal_error(&mut self, error: &str);
}

pub fn run(
    config: DesktopConfig,
    delegate: impl DesktopDelegate,
    platform: impl PlatformServices + 'static,
) -> Result<()> {
    let event_loop = EventLoop::<DesktopEvent>::with_user_event()
        .build()
        .context("failed to create the desktop event loop")?;
    let handle = DesktopHandle {
        proxy: event_loop.create_proxy(),
    };
    let proxy = handle.proxy.clone();
    let events: CefEventSender = Arc::new(move |event| {
        let _ = proxy.send_event(DesktopEvent::Browser(event));
    });
    let browser = CefBrowser::initialize(
        CefConfig {
            distribution: config.cef_distribution.clone(),
            cache_root: config.browser_cache_root.clone(),
            initial_url: config.initial_url.clone(),
            resources_root: config.browser_resources_root.clone(),
        },
        events,
    )
    .map_err(anyhow::Error::msg)
    .context("failed to initialize CEF")?;
    let mut runtime = DesktopRuntime {
        config,
        delegate: Box::new(delegate),
        platform: Box::new(platform),
        handle,
        browser,
        window: None,
        presenter: None,
        layout: None,
        cursor: (0.0, 0.0),
        visible: false,
        browser_ready: false,
        pending_web_messages: VecDeque::new(),
        last_window_state: None,
        input: InputState::default(),
    };
    let result = event_loop
        .run_app(&mut runtime)
        .context("desktop event loop failed");
    runtime.browser.shutdown();
    result
}

struct DesktopRuntime {
    config: DesktopConfig,
    delegate: Box<dyn DesktopDelegate>,
    platform: Box<dyn PlatformServices>,
    handle: DesktopHandle,
    browser: CefBrowser,
    window: Option<Arc<Window>>,
    presenter: Option<Presenter>,
    layout: Option<Layout>,
    cursor: (f64, f64),
    visible: bool,
    browser_ready: bool,
    pending_web_messages: VecDeque<WebMessage>,
    last_window_state: Option<WindowState>,
    input: InputState,
}

impl ApplicationHandler<DesktopEvent> for DesktopRuntime {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(error) = self.initialize(event_loop) {
            self.delegate.fatal_error(&format!("{error:#}"));
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DesktopEvent) {
        match event {
            DesktopEvent::Browser(event) => self.browser_event(event_loop, event),
            DesktopEvent::SendWebMessage(message) => {
                self.send_web_message(message);
            }
            DesktopEvent::Viewport {
                id,
                bounds,
                scale_factor,
                workspace_color,
            } => {
                let result = self.layout.as_mut().map_or_else(
                    || {
                        Err(crate::geometry::LayoutError::Runtime(
                            "desktop layout is not initialized".into(),
                        ))
                    },
                    |layout| layout.apply_current_viewport(bounds, scale_factor),
                );
                match &result {
                    Ok(viewport) => {
                        if let Some(presenter) = &mut self.presenter {
                            presenter.set_viewport(*viewport, workspace_color);
                        }
                        self.request_redraw();
                    }
                    Err(error) => tracing::debug!(?error, "ignored invalid or stale viewport"),
                }
                if let Some(presenter) = &mut self.presenter {
                    self.delegate
                        .viewport_applied(id, result, presenter, &self.handle);
                }
            }
            DesktopEvent::Window { id, action } => {
                self.window_action(event_loop, id, action);
            }
            DesktopEvent::OpenExternal { id, url } => {
                let response = self.platform.open_external(&url);
                self.delegate.platform_response(id, response, &self.handle);
            }
            DesktopEvent::Wake => {
                if let Some(presenter) = &mut self.presenter {
                    self.delegate.wake(presenter, &self.handle);
                }
                self.request_redraw();
            }
            DesktopEvent::Shutdown => self.shutdown(event_loop),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.browser.pump();
        if self.presenter.as_ref().is_some_and(Presenter::needs_redraw) {
            self.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
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
        match event {
            WindowEvent::CloseRequested => self.shutdown(event_loop),
            WindowEvent::Resized(size) => {
                self.resize(size.width, size.height, None);
                self.notify_window_state();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    self.resize(size.width, size.height, Some(scale_factor));
                }
                self.notify_window_state();
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            WindowEvent::Focused(focused) => {
                self.send_input(InputEvent::Focus { focused });
                self.notify_window_state();
            }
            WindowEvent::Moved(_) | WindowEvent::Occluded(_) => self.notify_window_state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                self.send_input(InputEvent::PointerMoved {
                    x: position.x,
                    y: position.y,
                    modifiers: self.input.modifiers,
                });
            }
            WindowEvent::CursorLeft { .. } => self.send_input(InputEvent::PointerLeft {
                x: self.cursor.0,
                y: self.cursor.1,
                modifiers: self.input.modifiers,
            }),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = mouse_button(button);
                let state = button_state(state);
                self.input.apply_mouse_button(button, state);
                let click_count =
                    self.input
                        .click_count(state, button, self.cursor, Instant::now());
                self.send_input(InputEvent::MouseButton {
                    state,
                    button,
                    x: self.cursor.0,
                    y: self.cursor.1,
                    modifiers: self.input.modifiers,
                    click_count,
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.send_input(InputEvent::Scroll {
                    delta: scroll_delta(delta),
                    x: self.cursor.0,
                    y: self.cursor.1,
                    modifiers: self.input.modifiers,
                });
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.input.apply_keyboard_modifiers(modifiers.state());
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.input
                    .apply_modifier_key(&event.logical_key, key_state(event.state));
                let unmodified = event.key_without_modifiers();
                self.send_input(InputEvent::Key {
                    state: key_state(event.state),
                    windows_key_code: virtual_key_code(&event.logical_key),
                    platform_key_code: platform_key_code(event.physical_key),
                    character: character(&event.logical_key),
                    unmodified_character: character(&unmodified),
                    text: event
                        .text
                        .as_deref()
                        .unwrap_or_default()
                        .encode_utf16()
                        .collect(),
                    repeat: event.repeat,
                    modifiers: self.input.modifiers,
                    location: key_location(event.location),
                });
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                self.send_input(InputEvent::ImePreedit {
                    text: text.into(),
                    cursor,
                });
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.send_input(InputEvent::ImeCommit { text: text.into() });
            }
            WindowEvent::Ime(Ime::Enabled | Ime::Disabled) => {}
            _ => {}
        }
    }
}

impl DesktopRuntime {
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attributes = Window::default_attributes()
            .with_title(self.config.title.to_string())
            .with_window_icon(Some(decode_window_icon(self.config.icon_png)?))
            .with_inner_size(LogicalSize::new(
                self.config.initial_width,
                self.config.initial_height,
            ))
            .with_min_inner_size(LogicalSize::new(
                MINIMUM_WINDOW_WIDTH,
                MINIMUM_WINDOW_HEIGHT,
            ))
            .with_visible(false)
            .with_decorations(false)
            .with_transparent(false);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("failed to create the sole desktop window")?,
        );
        configure_window(&window);
        center_window(&window, event_loop);
        let size = window.inner_size();
        let layout = Layout::new(size.width, size.height, window.scale_factor())?;
        let wake_handle = self.handle.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = wake_handle.wake();
        });
        let presenter = pollster::block_on(Presenter::new(Arc::clone(&window), wake))?;
        self.browser
            .create(layout.metrics(), presenter.browser_gpu())
            .map_err(anyhow::Error::msg)
            .context("failed to create the windowless browser")?;
        self.layout = Some(layout);
        self.window = Some(window);
        self.presenter = Some(presenter);
        self.notify_window_state();
        self.request_redraw();
        Ok(())
    }

    fn browser_event(&mut self, event_loop: &ActiveEventLoop, event: CefEvent) {
        match event {
            CefEvent::Ready => {
                if self.browser_ready {
                    return;
                }
                self.browser_ready = true;
                while let Some(message) = self.pending_web_messages.pop_front() {
                    self.browser.send_web_message(message);
                }
                self.delegate.browser_ready(&self.handle);
            }
            CefEvent::InitializationFailed(reason) => {
                self.delegate.fatal_error(&reason);
                self.shutdown(event_loop);
            }
            CefEvent::FrameAvailable => {
                let Some(frame) = self.browser.take_frame() else {
                    return;
                };
                if let Some(presenter) = &mut self.presenter {
                    presenter.offer_ui_frame(frame);
                }
                if !self.visible {
                    if let Some(window) = &self.window {
                        window.set_visible(true);
                    }
                    self.visible = true;
                }
                self.request_redraw();
            }
            CefEvent::AcceleratedPaintFailed(reason) => {
                tracing::warn!(%reason, "CEF accelerated OSR failed; recreating with software paint");
                let Some(view) = self.layout.as_ref().map(Layout::metrics) else {
                    self.delegate.fatal_error(&reason);
                    self.shutdown(event_loop);
                    return;
                };
                self.browser_ready = false;
                if let Err(error) = self.browser.fall_back_to_software(view) {
                    let error =
                        format!("failed to fall back from accelerated CEF rendering: {error}");
                    self.delegate.fatal_error(&error);
                    self.shutdown(event_loop);
                }
            }
            CefEvent::Cursor(cursor) => {
                if let Some(window) = &self.window {
                    match cursor {
                        BrowserCursor::Hidden => window.set_cursor_visible(false),
                        BrowserCursor::Icon(icon) => {
                            window.set_cursor(icon);
                            window.set_cursor_visible(true);
                        }
                    }
                }
            }
            CefEvent::WebMessage(message) => {
                if let Some(presenter) = &mut self.presenter {
                    self.delegate
                        .browser_message(message, presenter, &self.handle);
                    self.request_redraw();
                }
            }
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(presenter) = self.presenter.as_mut() else {
            return;
        };
        match presenter.present() {
            Ok(outcome) => {
                if outcome.needs_redraw {
                    self.request_redraw();
                }
            }
            Err(error) => {
                self.delegate.fatal_error(&error.to_string());
                self.shutdown(event_loop);
            }
        }
    }

    fn resize(&mut self, width: u32, height: u32, scale_factor: Option<f64>) {
        let Some(layout) = &mut self.layout else {
            return;
        };
        let scale_factor = scale_factor.unwrap_or(layout.metrics().scale_factor);
        match layout.resize(width, height, scale_factor) {
            Ok(true) => {
                let metrics = layout.metrics();
                if let Some(presenter) = &mut self.presenter {
                    presenter.resize(PhysicalSize::new(width, height));
                }
                self.browser.resize(metrics);
                self.request_redraw();
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(?error, "ignored invalid window metrics"),
        }
    }

    fn window_action(&mut self, event_loop: &ActiveEventLoop, id: u64, action: WindowAction) {
        let Some(window) = &self.window else {
            self.delegate.window_action_response(
                id,
                action,
                Err(WindowActionError::Rejected(
                    "desktop window is not initialized".into(),
                )),
                &self.handle,
            );
            return;
        };
        let result = match action {
            WindowAction::Minimize => {
                window.set_minimized(true);
                Ok(window_state(window))
            }
            WindowAction::ToggleMaximize => {
                window.set_maximized(!window.is_maximized());
                Ok(window_state(window))
            }
            WindowAction::BeginDrag => window
                .drag_window()
                .map(|()| window_state(window))
                .map_err(|error| WindowActionError::Rejected(error.to_string())),
            WindowAction::Close => Ok(window_state(window)),
        };
        self.delegate
            .window_action_response(id, action, result, &self.handle);
        self.notify_window_state();
        if action == WindowAction::Close {
            self.shutdown(event_loop);
        }
    }

    fn notify_window_state(&mut self) {
        let Some(window) = &self.window else { return };
        let state = window_state(window);
        if self.last_window_state == Some(state) {
            return;
        }
        self.last_window_state = Some(state);
        self.delegate.window_state_changed(state, &self.handle);
    }

    fn send_input(&mut self, event: InputEvent) {
        self.browser.apply_input(&event);
    }

    fn send_web_message(&mut self, message: WebMessage) {
        if self.browser_ready {
            self.browser.send_web_message(message);
        } else {
            self.pending_web_messages.push_back(message);
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn shutdown(&mut self, event_loop: &ActiveEventLoop) {
        self.browser.shutdown();
        event_loop.exit();
    }
}

fn decode_window_icon(png: &[u8]) -> Result<Icon> {
    let image = image::load_from_memory(png)
        .context("failed to decode the application icon")?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height)
        .context("failed to create the application icon")
}

#[cfg(target_os = "windows")]
fn configure_window(window: &Window) {
    use winit::platform::windows::{CornerPreference, WindowExtWindows};

    window.set_corner_preference(CornerPreference::Round);
    window.set_undecorated_shadow(true);
}

#[cfg(not(target_os = "windows"))]
fn configure_window(_window: &Window) {}

fn window_state(window: &Window) -> WindowState {
    let size = window.inner_size();
    WindowState {
        focused: window.has_focus(),
        minimized: window.is_minimized().unwrap_or(false),
        maximized: window.is_maximized(),
        fullscreen: window.fullscreen().is_some(),
        width: size.width,
        height: size.height,
        scale_factor: window.scale_factor(),
    }
}

#[derive(Default)]
struct InputState {
    modifiers: InputModifiers,
    last_click: Option<Click>,
}

struct Click {
    button: MouseButton,
    position: (f64, f64),
    at: Instant,
    count: u8,
}

impl InputState {
    fn apply_keyboard_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers.shift = modifiers.shift_key();
        self.modifiers.control = modifiers.control_key();
        self.modifiers.alt = modifiers.alt_key();
        self.modifiers.meta = modifiers.super_key();
    }

    fn apply_modifier_key(&mut self, key: &Key, state: KeyState) {
        let pressed = state == KeyState::Pressed;
        match key {
            Key::Named(NamedKey::Shift) => self.modifiers.shift = pressed,
            Key::Named(NamedKey::Control) => self.modifiers.control = pressed,
            Key::Named(NamedKey::Alt | NamedKey::AltGraph) => self.modifiers.alt = pressed,
            Key::Named(NamedKey::Meta | NamedKey::Super | NamedKey::Hyper) => {
                self.modifiers.meta = pressed;
            }
            _ => {}
        }
    }

    fn apply_mouse_button(&mut self, button: MouseButton, state: ButtonState) {
        let pressed = state == ButtonState::Pressed;
        match button {
            MouseButton::Left => self.modifiers.left_mouse = pressed,
            MouseButton::Middle => self.modifiers.middle_mouse = pressed,
            MouseButton::Right => self.modifiers.right_mouse = pressed,
            MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => {}
        }
    }

    fn click_count(
        &mut self,
        state: ButtonState,
        button: MouseButton,
        position: (f64, f64),
        now: Instant,
    ) -> u8 {
        if state == ButtonState::Released {
            return self
                .last_click
                .as_ref()
                .filter(|click| click.button == button)
                .map_or(1, |click| click.count);
        }
        let count = self.last_click.as_ref().map_or(1, |click| {
            let close_in_time =
                now.saturating_duration_since(click.at) <= Duration::from_millis(500);
            let close_in_space = (position.0 - click.position.0).abs() <= 4.
                && (position.1 - click.position.1).abs() <= 4.;
            if click.button == button && close_in_time && close_in_space {
                match click.count {
                    1 => 2,
                    2 => 3,
                    _ => 2,
                }
            } else {
                1
            }
        });
        self.last_click = Some(Click {
            button,
            position,
            at: now,
            count,
        });
        count
    }
}

fn key_location(location: WinitKeyLocation) -> KeyLocation {
    match location {
        WinitKeyLocation::Left => KeyLocation::Left,
        WinitKeyLocation::Right => KeyLocation::Right,
        WinitKeyLocation::Numpad => KeyLocation::Numpad,
        WinitKeyLocation::Standard => KeyLocation::Standard,
    }
}

fn platform_key_code(key: PhysicalKey) -> i32 {
    let scancode = key.to_scancode().unwrap_or_default();
    #[cfg(target_os = "linux")]
    let scancode = scancode.saturating_add(8);
    i32::try_from(scancode).unwrap_or_default()
}

fn character(key: &Key) -> u16 {
    match key {
        Key::Named(NamedKey::Tab) => b'\t' as u16,
        Key::Named(NamedKey::Enter) => b'\r' as u16,
        Key::Named(NamedKey::Backspace) => 0x08,
        Key::Named(NamedKey::Escape) => 0x1B,
        Key::Character(text) => text.encode_utf16().next().unwrap_or_default(),
        _ => 0,
    }
}

fn virtual_key_code(key: &Key) -> i32 {
    match key {
        Key::Named(key) => named_virtual_key(*key),
        Key::Character(text) => text.chars().next().map_or(0, character_virtual_key),
        _ => 0,
    }
}

fn named_virtual_key(key: NamedKey) -> i32 {
    match key {
        NamedKey::Alt => 0x12,
        NamedKey::AltGraph => 0xA5,
        NamedKey::CapsLock => 0x14,
        NamedKey::Control => 0x11,
        NamedKey::NumLock => 0x90,
        NamedKey::ScrollLock => 0x91,
        NamedKey::Shift => 0x10,
        NamedKey::Meta => 0x5B,
        NamedKey::Enter => 0x0D,
        NamedKey::Tab => 0x09,
        NamedKey::ArrowLeft => 0x25,
        NamedKey::ArrowUp => 0x26,
        NamedKey::ArrowRight => 0x27,
        NamedKey::ArrowDown => 0x28,
        NamedKey::End => 0x23,
        NamedKey::Home => 0x24,
        NamedKey::PageDown => 0x22,
        NamedKey::PageUp => 0x21,
        NamedKey::Backspace => 0x08,
        NamedKey::Delete => 0x2E,
        NamedKey::Insert => 0x2D,
        NamedKey::ContextMenu => 0x5D,
        NamedKey::Escape => 0x1B,
        NamedKey::Pause => 0x13,
        NamedKey::PrintScreen => 0x2C,
        NamedKey::AudioVolumeDown => 0xAE,
        NamedKey::AudioVolumeUp => 0xAF,
        NamedKey::AudioVolumeMute => 0xAD,
        NamedKey::BrowserBack => 0xA6,
        NamedKey::BrowserFavorites => 0xAB,
        NamedKey::BrowserForward => 0xA7,
        NamedKey::BrowserHome => 0xAC,
        NamedKey::BrowserRefresh => 0xA8,
        NamedKey::BrowserSearch => 0xAA,
        NamedKey::BrowserStop => 0xA9,
        NamedKey::F1 => 0x70,
        NamedKey::F2 => 0x71,
        NamedKey::F3 => 0x72,
        NamedKey::F4 => 0x73,
        NamedKey::F5 => 0x74,
        NamedKey::F6 => 0x75,
        NamedKey::F7 => 0x76,
        NamedKey::F8 => 0x77,
        NamedKey::F9 => 0x78,
        NamedKey::F10 => 0x79,
        NamedKey::F11 => 0x7A,
        NamedKey::F12 => 0x7B,
        _ => 0,
    }
}

fn character_virtual_key(character: char) -> i32 {
    match character {
        'a'..='z' | 'A'..='Z' => character.to_ascii_uppercase() as i32,
        '0'..='9' => character as i32,
        '!' => 0x31,
        '@' => 0x32,
        '#' => 0x33,
        '$' => 0x34,
        '%' => 0x35,
        '^' => 0x36,
        '&' => 0x37,
        '*' => 0x38,
        '(' => 0x39,
        ')' => 0x30,
        '`' | '~' => 0xC0,
        '-' | '_' => 0xBD,
        '=' | '+' => 0xBB,
        '[' | '{' => 0xDB,
        ']' | '}' => 0xDD,
        '\\' | '|' => 0xDC,
        ';' | ':' => 0xBA,
        ',' | '<' => 0xBC,
        '.' | '>' => 0xBE,
        '\'' | '"' => 0xDE,
        '/' | '?' => 0xBF,
        ' ' => 0x20,
        _ => 0,
    }
}

fn center_window(window: &Window, event_loop: &ActiveEventLoop) {
    let Some(monitor) = window
        .current_monitor()
        .or_else(|| event_loop.primary_monitor())
    else {
        return;
    };
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let window_size = window.outer_size();
    let x = monitor_position.x.saturating_add(
        i32::try_from(monitor_size.width.saturating_sub(window_size.width) / 2).unwrap_or(i32::MAX),
    );
    let y = monitor_position.y.saturating_add(
        i32::try_from(monitor_size.height.saturating_sub(window_size.height) / 2)
            .unwrap_or(i32::MAX),
    );
    window.set_outer_position(PhysicalPosition::new(x, y));
}

fn button_state(state: ElementState) -> ButtonState {
    match state {
        ElementState::Pressed => ButtonState::Pressed,
        ElementState::Released => ButtonState::Released,
    }
}

fn key_state(state: ElementState) -> KeyState {
    match state {
        ElementState::Pressed => KeyState::Pressed,
        ElementState::Released => KeyState::Released,
    }
}

fn mouse_button(button: WinitMouseButton) -> MouseButton {
    match button {
        WinitMouseButton::Left => MouseButton::Left,
        WinitMouseButton::Right => MouseButton::Right,
        WinitMouseButton::Middle => MouseButton::Middle,
        WinitMouseButton::Back => MouseButton::Back,
        WinitMouseButton::Forward => MouseButton::Forward,
        WinitMouseButton::Other(button) => MouseButton::Other(button),
    }
}

fn scroll_delta(delta: MouseScrollDelta) -> ScrollDelta {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x, y },
        MouseScrollDelta::PixelDelta(position) => ScrollDelta::Pixels {
            x: position.x,
            y: position.y,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_keys_cover_shortcuts_navigation_and_punctuation() {
        assert_eq!(virtual_key_code(&Key::Character("a".into())), 0x41);
        assert_eq!(virtual_key_code(&Key::Named(NamedKey::ArrowLeft)), 0x25);
        assert_eq!(character_virtual_key('?'), 0xBF);
    }

    #[test]
    fn click_count_is_bounded_by_time_position_and_button() {
        let mut input = InputState::default();
        let start = Instant::now();
        assert_eq!(
            input.click_count(ButtonState::Pressed, MouseButton::Left, (10., 10.), start,),
            1
        );
        assert_eq!(
            input.click_count(
                ButtonState::Pressed,
                MouseButton::Left,
                (12., 12.),
                start + Duration::from_millis(100),
            ),
            2
        );
        assert_eq!(
            input.click_count(
                ButtonState::Pressed,
                MouseButton::Right,
                (12., 12.),
                start + Duration::from_millis(200),
            ),
            1
        );
    }

    #[test]
    fn fourth_nearby_click_starts_a_new_double_click_sequence() {
        let mut input = InputState::default();
        let start = Instant::now();
        let counts = (0..4)
            .map(|index| {
                input.click_count(
                    ButtonState::Pressed,
                    MouseButton::Left,
                    (10., 10.),
                    start + Duration::from_millis(index * 50),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(counts, [1, 2, 3, 2]);
    }

    #[test]
    fn pointer_modifiers_follow_pressed_mouse_buttons() {
        let mut input = InputState::default();
        input.apply_mouse_button(MouseButton::Left, ButtonState::Pressed);
        assert!(input.modifiers.left_mouse);
        input.apply_mouse_button(MouseButton::Left, ButtonState::Released);
        assert!(!input.modifiers.left_mouse);
    }
}
