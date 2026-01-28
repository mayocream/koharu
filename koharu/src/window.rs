use std::sync::Arc;

use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event_loop::EventLoopWindowTarget,
    window::{Window, WindowBuilder},
};
use wry::{WebView, WebViewBuilder};

#[derive(Debug, Clone)]
pub enum AppEvent {
    ShowSplash { port: u16 },
    ShowMain { port: u16 },
    Exit,
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
    let webview = WebViewBuilder::new()
        .with_url(url)
        .build(&window)
        .expect("Failed to create webview");

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
    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_devtools(cfg!(debug_assertions))
        .build(&window)
        .expect("Failed to create webview");

    (window, webview)
}
