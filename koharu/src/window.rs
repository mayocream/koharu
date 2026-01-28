use std::sync::Arc;

use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event_loop::EventLoopWindowTarget,
    window::{Window, WindowBuilder},
};
use wry::{WebView, WebViewBuilder, WebViewId, http::Response as WryResponse};

use crate::assets::EmbeddedUi;

pub const CUSTOM_PROTOCOL: &str = "koharu";

#[derive(Debug, Clone)]
pub enum AppEvent {
    ShowMain { port: u16 },
    Exit,
}

/// Builds a URL for the custom protocol or dev server.
pub fn build_url(path: &str, port: Option<u16>, dev_url: Option<&str>) -> String {
    let default_base = format!("{CUSTOM_PROTOCOL}://localhost");
    let base = dev_url.unwrap_or(&default_base);
    let url = format!("{base}{path}");
    match port {
        Some(p) => format!("{url}?port={p}"),
        None => url,
    }
}

fn handle_custom_protocol(
    _webview_id: WebViewId,
    request: wry::http::Request<Vec<u8>>,
) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    let path = request.uri().path();
    let path = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    EmbeddedUi::get_with_mime(path)
        .or_else(|| EmbeddedUi::get_with_mime("index.html")) // SPA fallback
        .map(|(data, mime)| {
            WryResponse::builder()
                .status(200)
                .header("Content-Type", mime)
                .body(data.into())
                .unwrap()
        })
        .unwrap_or_else(|| {
            WryResponse::builder()
                .status(404)
                .body(b"Not Found".as_slice().into())
                .unwrap()
        })
}

fn center_window(window: &Window) {
    if let Some(monitor) = window.primary_monitor() {
        let monitor_size = monitor.size();
        let window_size = window.inner_size();
        let x = (monitor_size.width - window_size.width) / 2;
        let y = (monitor_size.height - window_size.height) / 2;
        window.set_outer_position(PhysicalPosition::new(x as i32, y as i32));
    }
}

fn create_webview(window: &Arc<Window>, url: &str, devtools: bool) -> WebView {
    WebViewBuilder::new()
        .with_custom_protocol(CUSTOM_PROTOCOL.into(), handle_custom_protocol)
        .with_url(url)
        .with_devtools(devtools)
        .build(window)
        .expect("Failed to create webview")
}

pub fn create_splashscreen(
    event_loop: &EventLoopWindowTarget<AppEvent>,
    url: &str,
) -> (Arc<Window>, WebView) {
    let window = WindowBuilder::new()
        .with_title("Koharu")
        .with_inner_size(LogicalSize::new(200.0, 150.0))
        .with_decorations(false)
        .with_resizable(false)
        .build(event_loop)
        .expect("Failed to create splashscreen");

    center_window(&window);
    let window = Arc::new(window);
    let webview = create_webview(&window, url, false);

    (window, webview)
}

pub fn create_main_window(
    event_loop: &EventLoopWindowTarget<AppEvent>,
    url: &str,
) -> (Arc<Window>, WebView) {
    #[allow(unused_mut)]
    let mut builder = WindowBuilder::new()
        .with_title("Koharu")
        .with_inner_size(LogicalSize::new(1024.0, 768.0))
        .with_min_inner_size(LogicalSize::new(900.0, 600.0))
        .with_visible(false)
        .with_decorations(true);

    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::WindowBuilderExtMacOS;
        builder = builder
            .with_titlebar_transparent(true)
            .with_title_hidden(true)
            .with_fullsize_content_view(true);
    }

    let window = builder
        .build(event_loop)
        .expect("Failed to create main window");

    center_window(&window);
    let window = Arc::new(window);
    let webview = create_webview(&window, url, cfg!(debug_assertions));

    (window, webview)
}
