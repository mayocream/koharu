use std::{
    ffi::c_int,
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use crate::{
    browser::{
        AcceleratedFrameImporter, BrowserCursor, BrowserFrame, BrowserFrameMailbox, BrowserGpu,
        DirtyRect, InputEvent, SoftwareFrame, WebMessage,
    },
    geometry::WindowMetrics,
};
use cef::rc::Rc as _;
use cef::{
    App, Browser, BrowserProcessHandler, Callback, CefString, Client, CursorType, DisplayHandler,
    Frame, ImplApp, ImplBinaryValue, ImplBrowser, ImplBrowserHost, ImplBrowserProcessHandler,
    ImplClient, ImplCommandLine, ImplDisplayHandler, ImplFrame, ImplListValue, ImplProcessMessage,
    ImplRenderHandler, ImplRenderProcessHandler, ImplRequest, ImplRequestContext,
    ImplRequestHandler, ImplResourceHandler, ImplResourceRequestHandler, ImplResponse,
    ImplSchemeHandlerFactory, ImplSchemeRegistrar, ImplV8Context, ImplV8Handler, ImplV8Value,
    LifeSpanHandler, LoadHandler, PaintElementType, ProcessId, ProcessMessage, Rect, RenderHandler,
    RenderProcessHandler, Request, RequestHandler, ResourceHandler, ResourceReadCallback,
    ResourceRequestHandler, Response, ReturnValue, SchemeHandlerFactory, SchemeRegistrar,
    V8Context, V8Handler, V8Propertyattribute, V8Value, WindowInfo, WrapApp,
    WrapBrowserProcessHandler, WrapClient, WrapDisplayHandler, WrapLifeSpanHandler,
    WrapLoadHandler, WrapRenderHandler, WrapRenderProcessHandler, WrapRequestHandler,
    WrapResourceHandler, WrapResourceRequestHandler, WrapSchemeHandlerFactory, WrapV8Handler,
};

use super::{
    CefEvent, CefEventSender,
    resource::{self, Resource, ResourceRoot},
};

macro_rules! cef_ref_counted {
    ($name:ident, $wrap:ident, $raw:ident, $object:ident $(, $field:ident)*) => {
        impl Clone for $name {
            fn clone(&self) -> Self {
                unsafe { (&mut *self.$object).interface.add_ref() };
                Self {
                    $object: self.$object,
                    $($field: self.$field.clone(),)*
                }
            }
        }
        impl cef::rc::Rc for $name {
            fn as_base(&self) -> &cef::sys::cef_base_ref_counted_t {
                unsafe { std::mem::transmute(&(*self.$object).cef_object) }
            }
        }
        impl $wrap for $name {
            fn wrap_rc(&mut self, object: *mut cef::rc::RcImpl<cef::sys::$raw, Self>) {
                self.$object = object;
            }
        }
    };
}

const SCHEME: &str = "koharu";
const DOMAIN: &str = "app";
const SEND_TO_BROWSER: &str = "koharu_send_to_browser";
const SEND_TO_RENDERER: &str = "koharu_send_to_renderer";
const BRIDGE_READY: &str = "koharu_bridge_ready";

#[derive(Clone)]
pub(super) struct BrowserDelegate(Arc<DelegateInner>);

struct DelegateInner {
    events: CefEventSender,
    view: Mutex<WindowMetrics>,
    next_frame: AtomicU64,
    frames: BrowserFrameMailbox,
    accelerated: Option<AcceleratedFrameImporter>,
    accelerated_failure_sent: AtomicBool,
    resources: Option<ResourceRoot>,
    development_origin: Option<String>,
}

impl BrowserDelegate {
    pub fn new(
        events: CefEventSender,
        view: WindowMetrics,
        resources: Option<ResourceRoot>,
        development_origin: Option<String>,
        frames: BrowserFrameMailbox,
        accelerated_gpu: Option<BrowserGpu>,
    ) -> Self {
        Self(Arc::new(DelegateInner {
            events,
            view: Mutex::new(view),
            next_frame: AtomicU64::new(1),
            frames,
            accelerated: accelerated_gpu.map(AcceleratedFrameImporter::new),
            accelerated_failure_sent: AtomicBool::new(false),
            resources,
            development_origin,
        }))
    }

    pub fn view(&self) -> WindowMetrics {
        self.0
            .view
            .lock()
            .map(|view| *view)
            .unwrap_or(WindowMetrics {
                generation: 1,
                width: 1,
                height: 1,
                scale_factor: 1.0,
            })
    }

    pub fn update_view(&self, view: WindowMetrics) {
        if let Ok(mut current) = self.0.view.lock()
            && view.generation >= current.generation
            && view.width > 0
            && view.height > 0
            && view.scale_factor.is_finite()
            && view.scale_factor > 0.0
        {
            *current = view;
        }
    }

    fn send(&self, event: CefEvent) {
        (self.0.events)(event);
    }

    fn next_frame(&self) -> Option<u64> {
        self.0
            .next_frame
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |sequence| {
                sequence.checked_add(1)
            })
            .ok()
    }

    fn submit_frame(&self, frame: impl Into<BrowserFrame>) {
        self.0.frames.submit(frame.into());
    }

    fn report_accelerated_failure(&self, error: impl std::fmt::Display) {
        if !self.0.accelerated_failure_sent.swap(true, Ordering::AcqRel) {
            self.send(CefEvent::AcceleratedPaintFailed(error.to_string().into()));
        }
    }

    fn load(&self, path: &str) -> Option<Resource> {
        self.0.resources.as_ref()?.load(path)
    }

    fn allowed_navigation(&self, url: &str) -> bool {
        let Ok(url) = url::Url::parse(url) else {
            return false;
        };
        is_primary_url(&url, self.0.development_origin.as_deref())
    }

    fn allowed_resource(&self, url: &str) -> bool {
        let Ok(url) = url::Url::parse(url) else {
            return false;
        };
        if is_primary_url(&url, self.0.development_origin.as_deref()) || url.scheme() == "data" {
            return true;
        }
        let Some(inner) = url.as_str().strip_prefix("blob:") else {
            return false;
        };
        url::Url::parse(inner)
            .is_ok_and(|inner| is_primary_url(&inner, self.0.development_origin.as_deref()))
    }
}

pub(super) struct BrowserContext {
    browser: Browser,
    _request_context: cef::RequestContext,
}

impl BrowserContext {
    pub fn create(
        delegate: BrowserDelegate,
        initial_url: &str,
        accelerated: bool,
    ) -> Result<Self, String> {
        let mut factory = SchemeHandlerFactory::new(SchemeFactory::new(delegate.clone()));
        let request_context_settings = cef::RequestContextSettings::default();
        let mut request_context =
            cef::request_context_create_context(Some(&request_context_settings), None)
                .ok_or_else(|| "CEF failed to create the browser request context".to_owned())?;
        if request_context.register_scheme_handler_factory(
            Some(&SCHEME.into()),
            Some(&DOMAIN.into()),
            Some(&mut factory),
        ) != 1
        {
            return Err("CEF failed to register the koharu://app resource factory".into());
        }
        let window_info = WindowInfo {
            windowless_rendering_enabled: 1,
            shared_texture_enabled: accelerated.into(),
            ..Default::default()
        };
        let browser_settings = cef::BrowserSettings {
            windowless_frame_rate: 60,
            background_color: 0,
            ..Default::default()
        };
        let mut client = Client::new(BrowserClient::new(delegate));
        let browser = cef::browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&initial_url.into()),
            Some(&browser_settings),
            None,
            Some(&mut request_context),
        )
        .expect("CEF failed to create the windowless browser");
        Ok(Self {
            browser,
            _request_context: request_context,
        })
    }

    pub fn resize(&self, delegate: &BrowserDelegate, metrics: WindowMetrics) {
        delegate.update_view(metrics);
        if let Some(host) = self.browser.host() {
            host.set_zoom_level(metrics.scale_factor.ln() / 1.2_f64.ln());
            host.was_resized();
            host.invalidate(PaintElementType::default());
        }
    }

    pub fn apply_input(&self, events: &[InputEvent]) {
        let Some(host) = self.browser.host() else {
            return;
        };
        for event in events {
            apply_input(&host, event);
        }
    }

    pub fn send_web_message(&self, message: WebMessage) {
        let Some(frame) = self.browser.main_frame() else {
            return;
        };
        let Some(mut process_message) = cef::process_message_create(Some(&SEND_TO_RENDERER.into()))
        else {
            return;
        };
        let Some(arguments) = process_message.argument_list() else {
            return;
        };
        let json = CefString::from(message.json.as_ref());
        arguments.set_string(0, Some(&json));
        let ids = message
            .attachments
            .iter()
            .map(|attachment| attachment.id())
            .collect::<Vec<_>>()
            .join("\u{1f}");
        arguments.set_string(1, Some(&ids.as_str().into()));
        for (index, attachment) in message.attachments.iter().enumerate() {
            let mut binary = cef::binary_value_create(Some(attachment.bytes()));
            arguments.set_binary(index + 2, binary.as_mut());
        }
        frame.send_process_message(
            cef::sys::cef_process_id_t::PID_RENDERER.into(),
            Some(&mut process_message),
        );
    }

    pub fn close(&self) {
        if let Some(host) = self.browser.host() {
            host.close_browser(1);
        }
    }
}

pub(super) fn browser_app() -> App {
    App::new(BrowserApp {
        object: std::ptr::null_mut(),
    })
}

pub(super) fn render_app() -> App {
    App::new(RenderApp {
        object: std::ptr::null_mut(),
        handler: RenderProcessHandler::new(RenderProcess::new()),
    })
}

pub(super) fn execute_helper_process() -> ExitCode {
    #[cfg(target_os = "macos")]
    if let Err(error) = load_macos_framework(true) {
        eprintln!("Koharu CEF helper failed to load the framework: {error}");
        return ExitCode::FAILURE;
    }
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
    let mut app = render_app();
    let code = cef::execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if code < 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::from(code as u8)
    }
}

#[cfg(target_os = "macos")]
pub(super) fn load_macos_framework(helper: bool) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to resolve the CEF executable: {error}"))?;
    let loader = cef::library_loader::LibraryLoader::new(&executable, helper);
    if !loader.load() {
        return Err(format!(
            "failed to load Chromium Embedded Framework relative to {}",
            executable.display()
        ));
    }
    // The framework must remain loaded until cef_shutdown/execute_process has
    // returned; process teardown releases it.
    std::mem::forget(loader);
    Ok(())
}

pub(super) fn register_scheme(registrar: Option<&mut SchemeRegistrar>) {
    let Some(registrar) = registrar else { return };
    let flags = cef::sys::cef_scheme_options_t::CEF_SCHEME_OPTION_STANDARD as i32
        | cef::sys::cef_scheme_options_t::CEF_SCHEME_OPTION_SECURE as i32
        | cef::sys::cef_scheme_options_t::CEF_SCHEME_OPTION_CORS_ENABLED as i32
        | cef::sys::cef_scheme_options_t::CEF_SCHEME_OPTION_FETCH_ENABLED as i32;
    registrar.add_custom_scheme(Some(&SCHEME.into()), flags);
}

struct BrowserApp {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_app_t, Self>,
}

impl ImplApp for BrowserApp {
    fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
        Some(BrowserProcessHandler::new(BrowserProcess::new()))
    }

    fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
        register_scheme(registrar);
    }

    fn on_before_command_line_processing(
        &self,
        _process_type: Option<&CefString>,
        command_line: Option<&mut cef::CommandLine>,
    ) {
        let Some(command_line) = command_line else {
            return;
        };
        for switch in [
            "no-first-run",
            "noerrdialogs",
            "no-default-browser-check",
            "mute-audio",
            "incognito",
            "disable-sync",
            "disable-geolocation",
            "disable-notifications",
            "disable-background-networking",
            "disable-component-update",
        ] {
            command_line.append_switch(Some(&switch.into()));
        }
        command_line
            .append_switch_with_value(Some(&"renderer-process-limit".into()), Some(&"1".into()));
        command_line.append_switch_with_value(
            Some(&"disable-blink-features".into()),
            Some(&"WebBluetooth,WebUSB,Serial".into()),
        );
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_app_t {
        self.object.cast()
    }
}

cef_ref_counted!(BrowserApp, WrapApp, _cef_app_t, object);

struct BrowserProcess {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_browser_process_handler_t, Self>,
}

impl BrowserProcess {
    fn new() -> Self {
        Self {
            object: std::ptr::null_mut(),
        }
    }
}

impl ImplBrowserProcessHandler for BrowserProcess {
    fn on_already_running_app_relaunch(
        &self,
        _command_line: Option<&mut cef::CommandLine>,
        _current_directory: Option<&CefString>,
    ) -> c_int {
        1
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_browser_process_handler_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    BrowserProcess,
    WrapBrowserProcessHandler,
    _cef_browser_process_handler_t,
    object
);

struct BrowserClient {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_client_t, Self>,
    delegate: BrowserDelegate,
    render: RenderHandler,
    display: DisplayHandler,
    request: RequestHandler,
    load: LoadHandler,
    ready_sent: Arc<AtomicBool>,
}

impl BrowserClient {
    fn new(delegate: BrowserDelegate) -> Self {
        Self {
            object: std::ptr::null_mut(),
            delegate: delegate.clone(),
            render: RenderHandler::new(Renderer::new(delegate.clone())),
            display: DisplayHandler::new(CursorDisplay::new(delegate.clone())),
            request: RequestHandler::new(RequestGuard::new(delegate.clone())),
            load: LoadHandler::new(Loaded::new(delegate)),
            ready_sent: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl ImplClient for BrowserClient {
    fn render_handler(&self) -> Option<RenderHandler> {
        Some(self.render.clone())
    }

    fn display_handler(&self) -> Option<DisplayHandler> {
        Some(self.display.clone())
    }

    fn request_handler(&self) -> Option<RequestHandler> {
        Some(self.request.clone())
    }

    fn load_handler(&self) -> Option<LoadHandler> {
        Some(self.load.clone())
    }

    fn life_span_handler(&self) -> Option<LifeSpanHandler> {
        Some(LifeSpanHandler::new(NoPopups::new()))
    }

    fn on_process_message_received(
        &self,
        _browser: Option<&mut Browser>,
        _frame: Option<&mut Frame>,
        _source_process: ProcessId,
        message: Option<&mut ProcessMessage>,
    ) -> c_int {
        let Some(message) = message else { return 0 };
        let name = userfree_to_string(message.name());
        if name == BRIDGE_READY {
            if !self.ready_sent.swap(true, Ordering::AcqRel) {
                self.delegate.send(CefEvent::Ready);
            }
            return 1;
        }
        if name != SEND_TO_BROWSER {
            return 0;
        }
        let Some(arguments) = message.argument_list() else {
            return 0;
        };
        let json = userfree_to_string(arguments.string(0));
        self.delegate
            .send(CefEvent::WebMessage(WebMessage::json(json)));
        1
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_client_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    BrowserClient,
    WrapClient,
    _cef_client_t,
    object,
    delegate,
    render,
    display,
    request,
    load,
    ready_sent
);

#[cfg(not(target_os = "macos"))]
type CefCursorHandle = cef::CursorHandle;
#[cfg(target_os = "macos")]
type CefCursorHandle = *mut u8;

struct CursorDisplay {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_display_handler_t, Self>,
    delegate: BrowserDelegate,
}

impl CursorDisplay {
    fn new(delegate: BrowserDelegate) -> Self {
        Self {
            object: std::ptr::null_mut(),
            delegate,
        }
    }
}

impl ImplDisplayHandler for CursorDisplay {
    fn on_cursor_change(
        &self,
        _browser: Option<&mut Browser>,
        _cursor: CefCursorHandle,
        type_: CursorType,
        _custom_cursor_info: Option<&cef::CursorInfo>,
    ) -> c_int {
        self.delegate.send(CefEvent::Cursor(cursor(type_)));
        1
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_display_handler_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    CursorDisplay,
    WrapDisplayHandler,
    _cef_display_handler_t,
    object,
    delegate
);

struct Renderer {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_render_handler_t, Self>,
    delegate: BrowserDelegate,
}

impl Renderer {
    fn new(delegate: BrowserDelegate) -> Self {
        Self {
            object: std::ptr::null_mut(),
            delegate,
        }
    }
}

impl ImplRenderHandler for Renderer {
    fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
        let Some(rect) = rect else { return };
        let view = self.delegate.view();
        *rect = Rect {
            x: 0,
            y: 0,
            width: view.width.min(i32::MAX as u32) as i32,
            height: view.height.min(i32::MAX as u32) as i32,
        };
    }

    fn on_paint(
        &self,
        _browser: Option<&mut Browser>,
        type_: PaintElementType,
        dirty_rects: Option<&[Rect]>,
        buffer: *const u8,
        width: c_int,
        height: c_int,
    ) {
        if type_ != PaintElementType::default() || width <= 0 || height <= 0 || buffer.is_null() {
            return;
        }
        let width = width as u32;
        let height = height as u32;
        if width > 32_768 || height > 32_768 {
            tracing::error!(
                width,
                height,
                "CEF OnPaint exceeded the supported frame dimensions"
            );
            return;
        }
        let Some(len) = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .filter(|len| *len <= isize::MAX as usize)
        else {
            return;
        };
        let pixels = Arc::<[u8]>::from(unsafe { std::slice::from_raw_parts(buffer, len) });
        let dirty = dirty_rects
            .unwrap_or_default()
            .iter()
            .filter_map(|rect| {
                (rect.x >= 0 && rect.y >= 0 && rect.width > 0 && rect.height > 0).then_some(
                    DirtyRect {
                        x: rect.x as u32,
                        y: rect.y as u32,
                        width: rect.width as u32,
                        height: rect.height as u32,
                    },
                )
            })
            .collect();
        let Some(sequence) = self.delegate.next_frame() else {
            return;
        };
        match SoftwareFrame::new(sequence, width, height, width * 4, dirty, pixels) {
            Ok(frame) => self.delegate.submit_frame(frame),
            Err(error) => tracing::error!(%error, "rejected invalid CEF software frame"),
        }
    }

    fn on_accelerated_paint(
        &self,
        _browser: Option<&mut Browser>,
        type_: PaintElementType,
        _dirty_rects: Option<&[Rect]>,
        info: Option<&cef::AcceleratedPaintInfo>,
    ) {
        if type_ != PaintElementType::default() {
            return;
        }
        let Some(info) = info else {
            self.delegate
                .report_accelerated_failure("CEF accelerated paint omitted its texture metadata");
            return;
        };
        let Some(importer) = self.delegate.0.accelerated.as_ref() else {
            self.delegate
                .report_accelerated_failure("CEF accelerated paint has no WGPU importer");
            return;
        };
        let Some(sequence) = self.delegate.next_frame() else {
            return;
        };
        match importer.import(sequence, info) {
            Ok(frame) => self.delegate.submit_frame(frame),
            Err(error) => self.delegate.report_accelerated_failure(error),
        }
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_render_handler_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    Renderer,
    WrapRenderHandler,
    _cef_render_handler_t,
    object,
    delegate
);

struct RequestGuard {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_request_handler_t, Self>,
    delegate: BrowserDelegate,
}

impl RequestGuard {
    fn new(delegate: BrowserDelegate) -> Self {
        Self {
            object: std::ptr::null_mut(),
            delegate,
        }
    }
}

impl ImplRequestHandler for RequestGuard {
    fn on_before_browse(
        &self,
        _browser: Option<&mut Browser>,
        _frame: Option<&mut Frame>,
        request: Option<&mut Request>,
        _user_gesture: c_int,
        _is_redirect: c_int,
    ) -> c_int {
        let Some(request) = request else { return 1 };
        (!self
            .delegate
            .allowed_navigation(&userfree_to_string(request.url()))) as c_int
    }

    fn resource_request_handler(
        &self,
        _browser: Option<&mut Browser>,
        _frame: Option<&mut Frame>,
        _request: Option<&mut Request>,
        _is_navigation: c_int,
        _is_download: c_int,
        _request_initiator: Option<&CefString>,
        _disable_default_handling: Option<&mut c_int>,
    ) -> Option<ResourceRequestHandler> {
        Some(ResourceRequestHandler::new(ResourceGuard::new(
            self.delegate.clone(),
        )))
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_request_handler_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    RequestGuard,
    WrapRequestHandler,
    _cef_request_handler_t,
    object,
    delegate
);

struct ResourceGuard {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_resource_request_handler_t, Self>,
    delegate: BrowserDelegate,
}

impl ResourceGuard {
    fn new(delegate: BrowserDelegate) -> Self {
        Self {
            object: std::ptr::null_mut(),
            delegate,
        }
    }
}

impl ImplResourceRequestHandler for ResourceGuard {
    fn on_before_resource_load(
        &self,
        _browser: Option<&mut Browser>,
        _frame: Option<&mut Frame>,
        request: Option<&mut Request>,
        _callback: Option<&mut Callback>,
    ) -> ReturnValue {
        let Some(request) = request else {
            return ReturnValue::CANCEL;
        };
        if self
            .delegate
            .allowed_resource(&userfree_to_string(request.url()))
        {
            ReturnValue::CONTINUE
        } else {
            ReturnValue::CANCEL
        }
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_resource_request_handler_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    ResourceGuard,
    WrapResourceRequestHandler,
    _cef_resource_request_handler_t,
    object,
    delegate
);

struct NoPopups {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_life_span_handler_t, Self>,
}

impl NoPopups {
    fn new() -> Self {
        Self {
            object: std::ptr::null_mut(),
        }
    }
}

impl cef::ImplLifeSpanHandler for NoPopups {
    fn on_before_popup(
        &self,
        _browser: Option<&mut Browser>,
        _frame: Option<&mut Frame>,
        _popup_id: c_int,
        _target_url: Option<&CefString>,
        _target_frame_name: Option<&CefString>,
        _target_disposition: cef::WindowOpenDisposition,
        _user_gesture: c_int,
        _popup_features: Option<&cef::PopupFeatures>,
        _window_info: Option<&mut WindowInfo>,
        _client: Option<&mut Option<Client>>,
        _settings: Option<&mut cef::BrowserSettings>,
        _extra_info: Option<&mut Option<cef::DictionaryValue>>,
        _no_javascript_access: Option<&mut c_int>,
    ) -> c_int {
        1
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_life_span_handler_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    NoPopups,
    WrapLifeSpanHandler,
    _cef_life_span_handler_t,
    object
);

struct Loaded {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_load_handler_t, Self>,
    delegate: BrowserDelegate,
}

impl Loaded {
    fn new(delegate: BrowserDelegate) -> Self {
        Self {
            object: std::ptr::null_mut(),
            delegate,
        }
    }
}

impl cef::ImplLoadHandler for Loaded {
    fn on_loading_state_change(
        &self,
        browser: Option<&mut Browser>,
        is_loading: c_int,
        _can_go_back: c_int,
        _can_go_forward: c_int,
    ) {
        if is_loading == 0
            && let Some(browser) = browser
            && let Some(host) = browser.host()
        {
            let view = self.delegate.view();
            host.set_zoom_level(view.scale_factor.ln() / 1.2_f64.ln());
        }
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_load_handler_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    Loaded,
    WrapLoadHandler,
    _cef_load_handler_t,
    object,
    delegate
);

pub(super) struct SchemeFactory {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_scheme_handler_factory_t, Self>,
    delegate: BrowserDelegate,
}

impl SchemeFactory {
    pub fn new(delegate: BrowserDelegate) -> Self {
        Self {
            object: std::ptr::null_mut(),
            delegate,
        }
    }
}

impl ImplSchemeHandlerFactory for SchemeFactory {
    fn create(
        &self,
        _browser: Option<&mut Browser>,
        _frame: Option<&mut Frame>,
        _scheme_name: Option<&CefString>,
        request: Option<&mut Request>,
    ) -> Option<ResourceHandler> {
        let request = request?;
        let url = url::Url::parse(&userfree_to_string(request.url())).ok()?;
        if !is_app_url(&url) {
            return None;
        }
        Some(ResourceHandler::new(FileResource::new(
            self.delegate.load(url.path().trim_start_matches('/')),
        )))
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_scheme_handler_factory_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    SchemeFactory,
    WrapSchemeHandlerFactory,
    _cef_scheme_handler_factory_t,
    object,
    delegate
);

struct FileResource {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_resource_handler_t, Self>,
    resource: Arc<Mutex<Option<Resource>>>,
}

impl FileResource {
    fn new(resource: Option<Resource>) -> Self {
        Self {
            object: std::ptr::null_mut(),
            resource: Arc::new(Mutex::new(resource)),
        }
    }
}

impl ImplResourceHandler for FileResource {
    fn open(
        &self,
        _request: Option<&mut Request>,
        handle_request: Option<&mut c_int>,
        _callback: Option<&mut Callback>,
    ) -> c_int {
        if let Some(handle_request) = handle_request {
            *handle_request = 1;
        }
        1
    }

    fn response_headers(
        &self,
        response: Option<&mut Response>,
        response_length: Option<&mut i64>,
        _redirect_url: Option<&mut CefString>,
    ) {
        if let Some(response_length) = response_length {
            *response_length = -1;
        }
        let Some(response) = response else { return };
        let Ok(resource) = self.resource.lock() else {
            response.set_status(500);
            return;
        };
        if let Some(resource) = resource.as_ref() {
            response.set_status(200);
            response.set_mime_type(Some(&resource.mime_type.into()));
        } else {
            response.set_status(404);
            response.set_mime_type(Some(&"text/plain".into()));
        }
    }

    fn read(
        &self,
        data_out: *mut u8,
        bytes_to_read: c_int,
        bytes_read: Option<&mut c_int>,
        _callback: Option<&mut ResourceReadCallback>,
    ) -> c_int {
        let Some(bytes_read) = bytes_read else {
            return 0;
        };
        if data_out.is_null() || bytes_to_read <= 0 {
            *bytes_read = 0;
            return 0;
        }
        let output = unsafe { std::slice::from_raw_parts_mut(data_out, bytes_to_read as usize) };
        let Ok(mut resource) = self.resource.lock() else {
            *bytes_read = -2;
            return 0;
        };
        let Some(resource) = resource.as_mut() else {
            *bytes_read = 0;
            return 0;
        };
        match resource::read(resource, output) {
            Ok(0) => {
                *bytes_read = 0;
                0
            }
            Ok(read) => {
                *bytes_read = read as c_int;
                1
            }
            Err(_) => {
                *bytes_read = -2;
                0
            }
        }
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_resource_handler_t {
        self.object.cast()
    }
}

impl Clone for FileResource {
    fn clone(&self) -> Self {
        unsafe { (&mut *self.object).interface.add_ref() };
        Self {
            object: self.object,
            resource: Arc::clone(&self.resource),
        }
    }
}

impl cef::rc::Rc for FileResource {
    fn as_base(&self) -> &cef::sys::cef_base_ref_counted_t {
        unsafe { std::mem::transmute(&(*self.object).cef_object) }
    }
}

impl WrapResourceHandler for FileResource {
    fn wrap_rc(&mut self, object: *mut cef::rc::RcImpl<cef::sys::_cef_resource_handler_t, Self>) {
        self.object = object;
    }
}

struct RenderApp {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_app_t, Self>,
    handler: RenderProcessHandler,
}

impl ImplApp for RenderApp {
    fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
        register_scheme(registrar);
    }

    fn render_process_handler(&self) -> Option<RenderProcessHandler> {
        Some(self.handler.clone())
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_app_t {
        self.object.cast()
    }
}

cef_ref_counted!(RenderApp, WrapApp, _cef_app_t, object, handler);

struct RenderProcess {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_render_process_handler_t, Self>,
}

impl RenderProcess {
    fn new() -> Self {
        Self {
            object: std::ptr::null_mut(),
        }
    }
}

impl ImplRenderProcessHandler for RenderProcess {
    fn on_context_created(
        &self,
        _browser: Option<&mut Browser>,
        frame: Option<&mut Frame>,
        context: Option<&mut V8Context>,
    ) {
        let (Some(frame), Some(context)) = (frame, context) else {
            return;
        };
        if frame.is_main() == 0 {
            return;
        }
        let Some(global) = context.global() else {
            return;
        };
        let Some(mut bridge) = cef::v8_value_create_object(None, None) else {
            return;
        };
        let mut handler = V8Handler::new(PostMessageHandler::new());
        let Some(mut post) =
            cef::v8_value_create_function(Some(&"postMessage".into()), Some(&mut handler))
        else {
            return;
        };
        bridge.set_value_bykey(
            Some(&"postMessage".into()),
            Some(&mut post),
            V8Propertyattribute::default(),
        );
        global.set_value_bykey(
            Some(&"koharu".into()),
            Some(&mut bridge),
            V8Propertyattribute::default(),
        );
    }

    fn on_process_message_received(
        &self,
        _browser: Option<&mut Browser>,
        frame: Option<&mut Frame>,
        _source_process: ProcessId,
        message: Option<&mut ProcessMessage>,
    ) -> c_int {
        let (Some(frame), Some(message)) = (frame, message) else {
            return 0;
        };
        if userfree_to_string(message.name()) != SEND_TO_RENDERER {
            return 0;
        }
        let Some(context) = frame.v8_context() else {
            return 0;
        };
        if context.enter() == 0 {
            return 0;
        }
        let delivered = deliver_server_message(&context, message);
        context.exit();
        delivered as c_int
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_render_process_handler_t {
        self.object.cast()
    }
}

cef_ref_counted!(
    RenderProcess,
    WrapRenderProcessHandler,
    _cef_render_process_handler_t,
    object
);

struct PostMessageHandler {
    object: *mut cef::rc::RcImpl<cef::sys::_cef_v8_handler_t, Self>,
}

impl PostMessageHandler {
    fn new() -> Self {
        Self {
            object: std::ptr::null_mut(),
        }
    }
}

impl ImplV8Handler for PostMessageHandler {
    fn execute(
        &self,
        name: Option<&CefString>,
        _object: Option<&mut V8Value>,
        arguments: Option<&[Option<V8Value>]>,
        _retval: Option<&mut Option<V8Value>>,
        exception: Option<&mut CefString>,
    ) -> c_int {
        if name.is_none_or(|name| name.to_string() != "postMessage") {
            return 0;
        }
        let Some(Some(value)) = arguments.and_then(|arguments| arguments.first()) else {
            if let Some(exception) = exception {
                *exception = "postMessage requires a ClientRequest".into();
            }
            return 1;
        };
        let Some(context) = cef::v8_context_get_current_context() else {
            return 0;
        };
        let Some(frame) = context.frame() else {
            return 0;
        };
        let Some(stringified) = call_json(&context, "stringify", value.clone()) else {
            if let Some(exception) = exception {
                *exception = "ClientRequest must be JSON serializable".into();
            }
            return 1;
        };
        if stringified.is_string() == 0 {
            if let Some(exception) = exception {
                *exception = "ClientRequest must serialize to a JSON value".into();
            }
            return 1;
        }
        let json = userfree_to_string(stringified.string_value());
        // Readiness follows the first request, after BrowserTransport has
        // installed receiveServerMessage. CEF preserves process-message order,
        // so startup messages cannot overtake the request that made the
        // bridge usable.
        if let Some(mut ready) = cef::process_message_create(Some(&BRIDGE_READY.into())) {
            frame.send_process_message(
                cef::sys::cef_process_id_t::PID_BROWSER.into(),
                Some(&mut ready),
            );
        }
        if let Some(mut message) = cef::process_message_create(Some(&SEND_TO_BROWSER.into())) {
            if let Some(list) = message.argument_list() {
                list.set_string(0, Some(&json.as_str().into()));
            }
            frame.send_process_message(
                cef::sys::cef_process_id_t::PID_BROWSER.into(),
                Some(&mut message),
            );
        }
        1
    }

    fn get_raw(&self) -> *mut cef::sys::_cef_v8_handler_t {
        self.object.cast()
    }
}

cef_ref_counted!(PostMessageHandler, WrapV8Handler, _cef_v8_handler_t, object);

fn deliver_server_message(context: &V8Context, message: &ProcessMessage) -> bool {
    let Some(arguments) = message.argument_list() else {
        return false;
    };
    let Some(global) = context.global() else {
        return false;
    };
    let Some(bridge) = global.value_bykey(Some(&"koharu".into())) else {
        return false;
    };
    let Some(callback) = bridge.value_bykey(Some(&"receiveServerMessage".into())) else {
        return false;
    };
    let json = userfree_to_string(arguments.string(0));
    let ids = userfree_to_string(arguments.string(1));
    let Some(json_text) = cef::v8_value_create_string(Some(&json.as_str().into())) else {
        return false;
    };
    let Some(json_value) = call_json(context, "parse", json_text) else {
        return false;
    };
    let Some(attachments) = cef::v8_value_create_object(None, None) else {
        return false;
    };
    for (index, id) in ids.split('\u{1f}').filter(|id| !id.is_empty()).enumerate() {
        let Some(binary) = arguments.binary(index + 2) else {
            return false;
        };
        let size = binary.size();
        let mut bytes = vec![0_u8; size];
        if binary.data(Some(&mut bytes), 0) != size {
            return false;
        }
        let Some(mut buffer) =
            cef::v8_value_create_array_buffer_with_copy(bytes.as_mut_ptr(), bytes.len())
        else {
            return false;
        };
        attachments.set_value_bykey(
            Some(&id.into()),
            Some(&mut buffer),
            V8Propertyattribute::default(),
        );
    }
    callback
        .execute_function(
            Some(&mut bridge.clone()),
            Some(&[Some(json_value), Some(attachments)]),
        )
        .is_some()
}

fn call_json(context: &V8Context, method: &str, argument: V8Value) -> Option<V8Value> {
    let global = context.global()?;
    let mut json = global.value_bykey(Some(&"JSON".into()))?;
    let function = json.value_bykey(Some(&method.into()))?;
    function.execute_function(Some(&mut json), Some(&[Some(argument)]))
}

fn apply_input(host: &cef::BrowserHost, event: &InputEvent) {
    match event {
        InputEvent::Focus { focused } => host.set_focus(*focused as c_int),
        InputEvent::PointerMoved { x, y, modifiers } => host.send_mouse_move_event(
            Some(&cef::MouseEvent {
                x: coordinate(*x),
                y: coordinate(*y),
                modifiers: cef_modifiers(*modifiers, false, crate::browser::KeyLocation::Standard),
            }),
            0,
        ),
        InputEvent::PointerLeft { x, y, modifiers } => host.send_mouse_move_event(
            Some(&cef::MouseEvent {
                x: coordinate(*x),
                y: coordinate(*y),
                modifiers: cef_modifiers(*modifiers, false, crate::browser::KeyLocation::Standard),
            }),
            1,
        ),
        InputEvent::MouseButton {
            state,
            button,
            x,
            y,
            modifiers,
            click_count,
        } => {
            let Some(button) = (match button {
                crate::browser::MouseButton::Left => {
                    Some(cef::sys::cef_mouse_button_type_t::MBT_LEFT)
                }
                crate::browser::MouseButton::Right => {
                    Some(cef::sys::cef_mouse_button_type_t::MBT_RIGHT)
                }
                crate::browser::MouseButton::Middle => {
                    Some(cef::sys::cef_mouse_button_type_t::MBT_MIDDLE)
                }
                _ => None,
            }) else {
                return;
            };
            host.send_mouse_click_event(
                Some(&cef::MouseEvent {
                    x: coordinate(*x),
                    y: coordinate(*y),
                    modifiers: cef_modifiers(
                        *modifiers,
                        false,
                        crate::browser::KeyLocation::Standard,
                    ),
                }),
                button.into(),
                matches!(state, crate::browser::ButtonState::Released) as c_int,
                i32::from(*click_count),
            );
        }
        InputEvent::Scroll {
            delta,
            x,
            y,
            modifiers,
        } => {
            let (delta_x, delta_y) = cef_scroll_delta(*delta);
            host.send_mouse_wheel_event(
                Some(&cef::MouseEvent {
                    x: coordinate(*x),
                    y: coordinate(*y),
                    modifiers: cef_modifiers(
                        *modifiers,
                        false,
                        crate::browser::KeyLocation::Standard,
                    ),
                }),
                delta_x,
                delta_y,
            );
        }
        InputEvent::Key {
            state,
            windows_key_code,
            platform_key_code,
            character,
            unmodified_character,
            text,
            repeat,
            modifiers,
            location,
        } => {
            let type_ = match state {
                crate::browser::KeyState::Pressed => {
                    cef::sys::cef_key_event_type_t::KEYEVENT_RAWKEYDOWN
                }
                crate::browser::KeyState::Released => {
                    cef::sys::cef_key_event_type_t::KEYEVENT_KEYUP
                }
            };
            host.send_key_event(Some(&cef::KeyEvent {
                type_: type_.into(),
                windows_key_code: *windows_key_code,
                native_key_code: *platform_key_code,
                modifiers: cef_modifiers(*modifiers, *repeat, *location),
                character: *character,
                unmodified_character: *unmodified_character,
                ..Default::default()
            }));
            if matches!(state, crate::browser::KeyState::Pressed) {
                for character in text.iter().copied() {
                    host.send_key_event(Some(&cef::KeyEvent {
                        type_: cef::sys::cef_key_event_type_t::KEYEVENT_CHAR.into(),
                        windows_key_code: *windows_key_code,
                        native_key_code: *platform_key_code,
                        modifiers: cef_modifiers(*modifiers, *repeat, *location),
                        character,
                        unmodified_character: *unmodified_character,
                        ..Default::default()
                    }));
                }
            }
        }
        InputEvent::ImePreedit { text, cursor } => {
            let selection = cursor.map_or(cef::Range::default(), |(from, to)| cef::Range {
                from: from.min(u32::MAX as usize) as u32,
                to: to.min(u32::MAX as usize) as u32,
            });
            host.ime_set_composition(Some(&text.as_ref().into()), None, None, Some(&selection));
        }
        InputEvent::ImeCommit { text } => {
            host.ime_commit_text(Some(&text.as_ref().into()), None, 0);
        }
    }
}

fn cef_scroll_delta(delta: crate::browser::ScrollDelta) -> (i32, i32) {
    // CEF's Windows OSR client forwards GET_WHEEL_DELTA_WPARAM unchanged.
    // Winit normalizes that native 120-unit notch to one LineDelta.
    #[cfg(target_os = "windows")]
    const UNITS_PER_LINE: f64 = 120.;
    #[cfg(not(target_os = "windows"))]
    const UNITS_PER_LINE: f64 = 40.;

    let (x, y) = match delta {
        crate::browser::ScrollDelta::Lines { x, y } => {
            (f64::from(x) * UNITS_PER_LINE, f64::from(y) * UNITS_PER_LINE)
        }
        crate::browser::ScrollDelta::Pixels { x, y } => (x, y),
    };
    (coordinate(x), coordinate(y))
}

fn coordinate(value: f64) -> i32 {
    value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn is_app_url(url: &url::Url) -> bool {
    url.scheme() == SCHEME
        && url.host_str() == Some(DOMAIN)
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
}

fn is_primary_url(url: &url::Url, development_origin: Option<&str>) -> bool {
    is_app_url(url)
        || development_origin.is_some_and(|origin| url.origin().ascii_serialization() == origin)
}

fn cursor(type_: CursorType) -> BrowserCursor {
    use winit::window::CursorIcon;

    let icon = match type_ {
        CursorType::NONE => return BrowserCursor::Hidden,
        CursorType::POINTER => CursorIcon::Default,
        CursorType::CROSS => CursorIcon::Crosshair,
        CursorType::HAND => CursorIcon::Pointer,
        CursorType::IBEAM => CursorIcon::Text,
        CursorType::WAIT => CursorIcon::Wait,
        CursorType::HELP => CursorIcon::Help,
        CursorType::EASTRESIZE => CursorIcon::EResize,
        CursorType::NORTHRESIZE => CursorIcon::NResize,
        CursorType::NORTHEASTRESIZE => CursorIcon::NeResize,
        CursorType::NORTHWESTRESIZE => CursorIcon::NwResize,
        CursorType::SOUTHRESIZE => CursorIcon::SResize,
        CursorType::SOUTHEASTRESIZE => CursorIcon::SeResize,
        CursorType::SOUTHWESTRESIZE => CursorIcon::SwResize,
        CursorType::WESTRESIZE => CursorIcon::WResize,
        CursorType::NORTHSOUTHRESIZE => CursorIcon::NsResize,
        CursorType::EASTWESTRESIZE => CursorIcon::EwResize,
        CursorType::NORTHEASTSOUTHWESTRESIZE => CursorIcon::NeswResize,
        CursorType::NORTHWESTSOUTHEASTRESIZE => CursorIcon::NwseResize,
        CursorType::COLUMNRESIZE => CursorIcon::ColResize,
        CursorType::ROWRESIZE => CursorIcon::RowResize,
        CursorType::MIDDLEPANNING
        | CursorType::EASTPANNING
        | CursorType::NORTHPANNING
        | CursorType::NORTHEASTPANNING
        | CursorType::NORTHWESTPANNING
        | CursorType::SOUTHPANNING
        | CursorType::SOUTHEASTPANNING
        | CursorType::SOUTHWESTPANNING
        | CursorType::WESTPANNING
        | CursorType::MIDDLE_PANNING_VERTICAL
        | CursorType::MIDDLE_PANNING_HORIZONTAL => CursorIcon::AllScroll,
        CursorType::MOVE | CursorType::DND_MOVE => CursorIcon::Move,
        CursorType::VERTICALTEXT => CursorIcon::VerticalText,
        CursorType::CELL => CursorIcon::Cell,
        CursorType::CONTEXTMENU => CursorIcon::ContextMenu,
        CursorType::ALIAS | CursorType::DND_LINK => CursorIcon::Alias,
        CursorType::PROGRESS => CursorIcon::Progress,
        CursorType::NODROP => CursorIcon::NoDrop,
        CursorType::COPY | CursorType::DND_COPY => CursorIcon::Copy,
        CursorType::NOTALLOWED => CursorIcon::NotAllowed,
        CursorType::ZOOMIN => CursorIcon::ZoomIn,
        CursorType::ZOOMOUT => CursorIcon::ZoomOut,
        CursorType::GRAB => CursorIcon::Grab,
        CursorType::GRABBING => CursorIcon::Grabbing,
        _ => CursorIcon::Default,
    };
    BrowserCursor::Icon(icon)
}

fn cef_modifiers(
    modifiers: crate::browser::InputModifiers,
    repeat: bool,
    location: crate::browser::KeyLocation,
) -> u32 {
    let mut flags = 0;
    if modifiers.shift {
        flags |= 1 << 1;
    }
    if modifiers.control {
        flags |= 1 << 2;
    }
    if modifiers.alt {
        flags |= 1 << 3;
    }
    if modifiers.meta {
        flags |= 1 << 7;
    }
    if modifiers.left_mouse {
        flags |= 1 << 4;
    }
    if modifiers.middle_mouse {
        flags |= 1 << 5;
    }
    if modifiers.right_mouse {
        flags |= 1 << 6;
    }
    if repeat {
        flags |= 1 << 13;
    }
    flags |= match location {
        crate::browser::KeyLocation::Left => 1 << 10,
        crate::browser::KeyLocation::Right => 1 << 11,
        crate::browser::KeyLocation::Numpad => 1 << 9,
        crate::browser::KeyLocation::Standard => 0,
    };
    flags
}

fn userfree_to_string(value: cef::CefStringUserfree) -> String {
    CefString::from(&value).to_string()
}

#[cfg(test)]
mod tests {
    use super::{cef_modifiers, cef_scroll_delta, cursor, is_app_url, is_primary_url};
    use crate::browser::{BrowserCursor, InputModifiers, KeyLocation, ScrollDelta};
    use cef::CursorType;
    use winit::window::CursorIcon;

    #[test]
    fn app_scheme_rejects_lookalike_authorities() {
        assert!(is_app_url(
            &url::Url::parse("koharu://app/index.html").unwrap()
        ));
        assert!(!is_app_url(
            &url::Url::parse("koharu://user@app/index.html").unwrap()
        ));
        assert!(!is_app_url(
            &url::Url::parse("koharu://app.evil/index.html").unwrap()
        ));
    }

    #[test]
    fn primary_url_requires_exact_development_origin() {
        let origin = Some("http://localhost:5173");
        assert!(is_primary_url(
            &url::Url::parse("http://localhost:5173/chunk.js").unwrap(),
            origin
        ));
        assert!(!is_primary_url(
            &url::Url::parse("http://localhost.evil:5173/chunk.js").unwrap(),
            origin
        ));
        assert!(is_primary_url(
            &url::Url::parse("http://user@localhost:5173/chunk.js").unwrap(),
            origin
        ));
    }

    #[test]
    fn cef_flags_preserve_mouse_state_repeat_and_key_location() {
        let flags = cef_modifiers(
            InputModifiers {
                control: true,
                left_mouse: true,
                ..InputModifiers::default()
            },
            true,
            KeyLocation::Numpad,
        );
        assert_ne!(flags & (1 << 2), 0);
        assert_ne!(flags & (1 << 4), 0);
        assert_ne!(flags & (1 << 9), 0);
        assert_ne!(flags & (1 << 13), 0);
    }

    #[test]
    fn cef_scroll_preserves_pixels_and_uses_platform_line_units() {
        assert_eq!(
            cef_scroll_delta(ScrollDelta::Pixels { x: 3.4, y: -8.6 }),
            (3, -9)
        );
        #[cfg(target_os = "windows")]
        assert_eq!(
            cef_scroll_delta(ScrollDelta::Lines { x: -0.5, y: 1. }),
            (-60, 120)
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            cef_scroll_delta(ScrollDelta::Lines { x: -0.5, y: 1. }),
            (-20, 40)
        );
    }

    #[test]
    fn cef_cursor_types_map_to_winit_and_hide_none() {
        assert_eq!(
            cursor(CursorType::HAND),
            BrowserCursor::Icon(CursorIcon::Pointer)
        );
        assert_eq!(
            cursor(CursorType::NORTHWESTSOUTHEASTRESIZE),
            BrowserCursor::Icon(CursorIcon::NwseResize)
        );
        assert_eq!(cursor(CursorType::NONE), BrowserCursor::Hidden);
        assert_eq!(
            cursor(CursorType::CUSTOM),
            BrowserCursor::Icon(CursorIcon::Default)
        );
    }
}
