use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    },
};

use anyhow::{Context, Result};
use clap::Parser;
use koharu_ml::{DeviceName, cuda_is_available, device_name};
use koharu_runtime::{ensure_dylibs, preload_dylibs};
use once_cell::sync::Lazy;
use rfd::MessageDialog;
use tauri::Manager;
use tokio::{net::TcpListener, sync::RwLock};
use tracing_subscriber::fmt::format::FmtSpan;

use crate::{
    command, llm, ml,
    renderer::Renderer,
    server,
    state::{AppState, State},
};

#[cfg(not(target_os = "windows"))]
fn resolve_app_root() -> PathBuf {
    dirs::data_local_dir()
        .map(|path| path.join("Koharu"))
        .unwrap_or(PathBuf::from("."))
}

#[cfg(target_os = "windows")]
fn resolve_app_root() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    if let Some(parent_dir) = exe_dir.as_ref().and_then(|dir| dir.parent())
        && parent_dir.join(".portable").is_file()
    {
        return parent_dir.to_path_buf();
    }

    dirs::data_local_dir()
        .map(|path| path.join("Koharu"))
        .or(exe_dir)
        .unwrap_or(PathBuf::from("."))
}

static APP_ROOT: Lazy<PathBuf> = Lazy::new(resolve_app_root);
static LIB_ROOT: Lazy<PathBuf> = Lazy::new(|| APP_ROOT.join("libs"));
static MODEL_ROOT: Lazy<PathBuf> = Lazy::new(|| APP_ROOT.join("models"));

#[derive(Clone)]
pub struct AppResources {
    pub state: AppState,
    pub ml: Arc<ml::Model>,
    pub llm: Arc<llm::Model>,
    pub renderer: Arc<Renderer>,
    pub ml_device: DeviceName,
    pub pipeline: Arc<RwLock<Option<crate::pipeline::PipelineHandle>>>,
}

#[derive(Parser)]
#[command(version = crate::version::APP_VERSION, about)]
struct Cli {
    #[arg(
        short,
        long,
        help = "Download dynamic libraries and exit",
        default_value_t = false
    )]
    download: bool,
    #[arg(
        long,
        help = "Force using CPU even if GPU is available",
        default_value_t = false
    )]
    cpu: bool,
    #[arg(
        short,
        long,
        value_name = "PORT",
        help = "Bind the HTTP server to a specific port instead of a random port"
    )]
    port: Option<u16>,
    #[arg(
        long,
        help = "Run in headless mode without starting the GUI",
        default_value_t = false
    )]
    headless: bool,
    #[arg(
        long,
        help = "Enable debug mode with console output",
        default_value_t = false
    )]
    debug: bool,
}

fn initialize(headless: bool, _debug: bool) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        // hide console window in release mode and not headless
        if headless || _debug {
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

    // hook model cache dir
    koharu_ml::set_cache_dir(MODEL_ROOT.to_path_buf())?;

    if headless {
        std::panic::set_hook(Box::new(|info| {
            eprintln!("panic: {info}");
        }));
    } else {
        std::panic::set_hook(Box::new(|info| {
            let msg = info.to_string();
            MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Panic")
                .set_description(&msg)
                .show();
            std::process::exit(1);
        }));
    }

    #[cfg(feature = "bundle")]
    {
        // https://docs.velopack.io/integrating/overview#application-startup
        velopack::VelopackApp::build().run();
    }

    Ok(())
}

#[cfg(feature = "bundle")]
async fn update_app() -> Result<()> {
    use velopack::{UpdateCheck, UpdateManager, sources::HttpSource};

    let source = HttpSource::new("https://github.com/mayocream/koharu/releases/latest/download");
    let um = UpdateManager::new(source, None, None)?;

    if let UpdateCheck::UpdateAvailable(updates) = um.check_for_updates()? {
        um.download_updates(&updates, None)?;
        um.apply_updates_and_restart(&updates)?;
    }

    Ok(())
}

async fn prefetch() -> Result<()> {
    ensure_dylibs(LIB_ROOT.to_path_buf()).await?;
    ml::prefetch().await?;
    // Skip for now as it's too big
    // llm::prefetch().await?;

    Ok(())
}

async fn build_resources(cpu: bool, _register_file_assoc: bool) -> Result<AppResources> {
    if cuda_is_available() {
        ensure_dylibs(LIB_ROOT.to_path_buf())
            .await
            .context("Failed to ensure dynamic libraries")?;
        preload_dylibs(LIB_ROOT.to_path_buf()).context("Failed to preload dynamic libraries")?;

        #[cfg(target_os = "windows")]
        {
            if _register_file_assoc && let Err(err) = crate::windows::register_khr() {
                tracing::warn!(?err, "Failed to register .khr file association");
            }

            crate::windows::add_dll_directory(&LIB_ROOT).context("Failed to add DLL directory")?;
        }

        tracing::info!(
            "CUDA is available, loaded dynamic libraries from {:?}",
            *LIB_ROOT
        );
    }

    let ml_device = device_name(cpu);
    let ml = Arc::new(
        ml::Model::new(cpu)
            .await
            .context("Failed to initialize ML model")?,
    );
    let llm = Arc::new(llm::Model::new(cpu));
    let renderer = Arc::new(Renderer::new().context("Failed to initialize renderer")?);
    let state = Arc::new(RwLock::new(State::default()));

    Ok(AppResources {
        state,
        ml,
        llm,
        renderer,
        ml_device,
        pipeline: Arc::new(RwLock::new(None)),
    })
}

pub async fn run() -> Result<()> {
    let Cli {
        download,
        cpu,
        port,
        headless,
        debug,
    } = Cli::parse();

    initialize(headless, debug)?;

    if download {
        prefetch().await?;
        return Ok(());
    }

    // Spawn background update check and auto-apply
    #[cfg(feature = "bundle")]
    tokio::spawn(async move {
        if let Err(err) = update_app().await {
            tracing::error!("Auto-update failed: {err:#}");
        }
    });

    if headless {
        let resources = build_resources(cpu, false).await?;
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port.unwrap_or(0))).await?;

        let server_resources = resources.clone();
        tokio::spawn(async move {
            if let Err(err) = server::serve_with_listener(listener, server_resources).await {
                tracing::error!("HTTP server error: {err:#}");
            }
        });

        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            command::initialize,
            command::download_progress,
        ])
        .manage(AtomicU16::new(0))
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let resources = build_resources(cpu, true)
                    .await
                    .expect("failed to build app resources");
                let listener = TcpListener::bind(format!("127.0.0.1:{}", port.unwrap_or(0)))
                    .await
                    .expect("failed to bind HTTP server");
                let port = listener
                    .local_addr()
                    .expect("failed to get listener address")
                    .port();

                handle.state::<AtomicU16>().store(port, Ordering::SeqCst);

                let server_resources = resources.clone();
                tokio::spawn(async move {
                    server::serve_with_listener(listener, server_resources)
                        .await
                        .expect("failed to run HTTP server");
                });

                handle
                    .get_webview_window("splashscreen")
                    .expect("splashscreen window not found")
                    .close()
                    .ok();
                handle
                    .get_webview_window("main")
                    .expect("main window not found")
                    .show()
                    .ok();
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
