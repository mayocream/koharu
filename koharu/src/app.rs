use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::Parser;
use koharu_ml::{DeviceName, cuda_is_available, device_name};
use koharu_runtime::{ensure_dylibs, preload_dylibs};
use once_cell::sync::Lazy;
use rfd::MessageDialog;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::Window,
};
use tokio::{net::TcpListener, sync::RwLock};
use tracing_subscriber::fmt::format::FmtSpan;
use wry::WebView;

use crate::{
    llm, ml,
    renderer::Renderer,
    server,
    state::{AppState, State},
    window::{AppEvent, build_url, create_main_window, create_splashscreen},
};

static APP_ROOT: Lazy<PathBuf> = Lazy::new(resolve_app_root);
static LIB_ROOT: Lazy<PathBuf> = Lazy::new(|| APP_ROOT.join("libs"));
static MODEL_ROOT: Lazy<PathBuf> = Lazy::new(|| APP_ROOT.join("models"));

#[cfg(not(target_os = "windows"))]
fn resolve_app_root() -> PathBuf {
    dirs::data_local_dir()
        .map(|p| p.join("Koharu"))
        .unwrap_or_else(|| ".".into())
}

#[cfg(target_os = "windows")]
fn resolve_app_root() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    if let Some(parent) = exe_dir.as_ref().and_then(|d| d.parent()) {
        if parent.join(".portable").is_file() {
            return parent.to_path_buf();
        }
    }

    dirs::data_local_dir()
        .map(|p| p.join("Koharu"))
        .or(exe_dir)
        .unwrap_or_else(|| ".".into())
}

#[derive(Parser)]
#[command(version = crate::version::APP_VERSION, about)]
struct Cli {
    /// Download dynamic libraries and exit
    #[arg(short, long)]
    download: bool,
    /// Force using CPU even if GPU is available
    #[arg(long)]
    cpu: bool,
    /// HTTP server port
    #[arg(short = 'p', long)]
    port: Option<u16>,
    /// Run in headless mode without GUI
    #[arg(long)]
    headless: bool,
    /// Enable debug mode with console output
    #[arg(long)]
    debug: bool,
    /// Dev url (internal use only)
    #[arg(long, hide = true)]
    dev_url: Option<String>,
}

#[derive(Clone)]
pub struct AppResources {
    pub state: AppState,
    pub ml: Arc<ml::Model>,
    pub llm: Arc<llm::Model>,
    pub renderer: Arc<Renderer>,
    pub ml_device: DeviceName,
}

fn initialize(headless: bool, debug: bool) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        if headless || debug {
            crate::windows::create_console_window();
        }
        crate::windows::enable_ansi_support().ok();
    }

    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .with_env_filter(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    koharu_ml::set_cache_dir(MODEL_ROOT.to_path_buf())?;

    std::panic::set_hook(Box::new(move |info| {
        if headless {
            eprintln!("panic: {info}");
        } else {
            MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Panic")
                .set_description(&info.to_string())
                .show();
            std::process::exit(1);
        }
    }));

    #[cfg(feature = "bundle")]
    velopack::VelopackApp::build().run();

    Ok(())
}

async fn build_resources(cpu: bool, register_assoc: bool) -> Result<AppResources> {
    if cuda_is_available() {
        ensure_dylibs(LIB_ROOT.to_path_buf()).await?;
        preload_dylibs(LIB_ROOT.to_path_buf())?;

        #[cfg(target_os = "windows")]
        {
            if register_assoc {
                crate::windows::register_khr().ok();
            }
            crate::windows::add_dll_directory(&LIB_ROOT)?;
        }

        tracing::info!("CUDA available, loaded libraries from {:?}", *LIB_ROOT);
    }

    Ok(AppResources {
        ml_device: device_name(cpu),
        ml: Arc::new(ml::Model::new(cpu).await?),
        llm: Arc::new(llm::Model::new(cpu)),
        renderer: Arc::new(Renderer::new()?),
        state: Arc::new(RwLock::new(State::default())),
    })
}

#[cfg(feature = "bundle")]
async fn check_for_updates() {
    use velopack::{UpdateCheck, UpdateManager, sources::HttpSource};
    let result: Result<()> = (|| async {
        let source =
            HttpSource::new("https://github.com/mayocream/koharu/releases/latest/download");
        let um = UpdateManager::new(source, None, None)?;
        if let UpdateCheck::UpdateAvailable(u) = um.check_for_updates()? {
            um.download_updates(&u, None)?;
            um.apply_updates_and_restart(&u)?;
        }
        Ok(())
    })()
    .await;
    if let Err(e) = result {
        tracing::error!("Auto-update failed: {e:#}");
    }
}

async fn run_server(
    cpu: bool,
    port: u16,
    dev_url: Option<String>,
    proxy: EventLoopProxy<AppEvent>,
) -> Result<()> {
    #[cfg(feature = "bundle")]
    tokio::spawn(check_for_updates());

    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let actual_port = listener.local_addr()?.port();
    tracing::info!("HTTP server bound to port {actual_port}");

    // Build resources (splashscreen is already showing via custom protocol)
    let resources = build_resources(cpu, true).await?;

    // Resources ready, show main window
    let _ = proxy.send_event(AppEvent::ShowMain { port: actual_port });

    server::serve(listener, resources, dev_url).await
}

fn run_gui(cpu: bool, port: u16, dev_url: Option<String>) -> Result<()> {
    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Start server in background thread
    let proxy_clone = proxy.clone();
    let dev_url_clone = dev_url.clone();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .expect("Failed to create runtime")
            .block_on(async {
                if let Err(e) = run_server(cpu, port, dev_url_clone, proxy_clone).await {
                    panic!("Server failed: {e:#}");
                }
            });
    });

    let splash_url = build_url("/splashscreen", dev_url.as_deref());
    let (mut _main_win, mut _main_wv, mut splash_win, mut splash_wv) = (
        None::<Arc<Window>>,
        None::<WebView>,
        None::<Arc<Window>>,
        None::<WebView>,
    );

    #[allow(unused_assignments)]
    event_loop.run(move |event, elwt, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(tao::event::StartCause::Init) => {
                let (w, v) = create_splashscreen(elwt, &splash_url);
                (splash_win, splash_wv) = (Some(w), Some(v));
            }
            Event::UserEvent(AppEvent::ShowMain { port: _ }) => {
                // Close splashscreen
                drop(splash_wv.take());
                drop(splash_win.take());

                let url = build_url("/", dev_url.as_deref());
                let (w, v) = create_main_window(elwt, &url);
                w.set_visible(true);
                (_main_win, _main_wv) = (Some(w), Some(v));
            }
            Event::UserEvent(AppEvent::Exit)
            | Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *flow = ControlFlow::Exit;
            }
            _ => {}
        }
    })
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    initialize(cli.headless, cli.debug)?;

    if cli.download {
        ensure_dylibs(LIB_ROOT.to_path_buf()).await?;
        return ml::prefetch().await;
    }

    let port = cli.port.unwrap_or(0);

    if cli.headless {
        let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
        let resources = build_resources(cli.cpu, false).await?;
        return server::serve(listener, resources, cli.dev_url).await;
    }

    run_gui(cli.cpu, port, cli.dev_url)
}
