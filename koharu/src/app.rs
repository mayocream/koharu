use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use clap::Parser;
use once_cell::sync::Lazy;
use rfd::MessageDialog;
use tauri::{Manager, WebviewWindowBuilder};
use tokio::{net::TcpListener, sync::RwLock};
use tracing_subscriber::fmt::format::FmtSpan;

use koharu_llm::facade;
use koharu_ml::{cuda_is_available, device};
use koharu_pipeline::AppResources;
use koharu_renderer::facade::Renderer;
use koharu_rpc::{SharedResources, server};
use koharu_types::State;

static APP_ROOT: Lazy<PathBuf> = Lazy::new(|| {
    dirs::data_local_dir()
        .map(|path| path.join("Koharu"))
        .unwrap_or_default()
});
static MODEL_ROOT: Lazy<PathBuf> = Lazy::new(|| APP_ROOT.join("models"));

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
        let attached_to_parent = crate::windows::attach_parent_console();

        // In GUI release builds, prefer the parent terminal if one exists.
        // Only allocate a new console window for explicit console-oriented runs.
        if !attached_to_parent && (headless || _debug) {
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

    Ok(())
}

async fn prefetch() -> Result<()> {
    koharu_runtime::initialize()
        .await
        .context("Failed to initialize runtime packages")?;
    koharu_ml::facade::prefetch().await?;

    Ok(())
}

fn warning(headless: bool, title: &str, description: &str) {
    tracing::warn!("{description}");

    if headless {
        return;
    }

    MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(title)
        .set_description(description)
        .show();
}

async fn build_resources(cpu: bool, headless: bool) -> Result<AppResources> {
    let mut cpu = cpu;

    if !cpu && cuda_is_available() {
        match koharu_runtime::cuda_driver_version() {
            Ok(version) if version.supports_cuda_13_1() => {
                tracing::info!("NVIDIA driver reports CUDA {version} support");
            }
            Ok(version) => {
                warning(
                    headless,
                    "NVIDIA Driver Update Recommended",
                    &format!(
                        "Your NVIDIA driver only supports CUDA {version}. Koharu will fall back to CPU. Please update your NVIDIA driver to a version that supports CUDA 13.1 or newer to enable GPU acceleration."
                    ),
                );
                cpu = true;
            }
            Err(err) => {
                warning(
                    headless,
                    "NVIDIA Driver Check Failed",
                    &format!(
                        "Koharu could not verify NVIDIA driver support for CUDA 13.1: {err:#}. Koharu will fall back to CPU. Please update your NVIDIA driver to a version that supports CUDA 13.1 or newer to enable GPU acceleration."
                    ),
                );
                cpu = true;
            }
        }
    }

    koharu_runtime::initialize()
        .await
        .context("Failed to initialize runtime packages")?;

    if !cpu && cuda_is_available() {
        #[cfg(target_os = "windows")]
        {
            if let Err(err) = crate::windows::register_khr() {
                tracing::warn!(?err, "Failed to register .khr file association");
            }
        }

        tracing::info!("CUDA is available and runtime packages were initialized");
    }

    let ml = Arc::new(
        koharu_ml::facade::Model::new(cpu)
            .await
            .context("Failed to initialize ML model")?,
    );
    let llm = Arc::new(facade::Model::new(cpu));
    let renderer = Arc::new(Renderer::new().context("Failed to initialize renderer")?);
    let state = Arc::new(RwLock::new(State::default()));

    Ok(AppResources {
        state,
        ml,
        llm,
        renderer,
        device: device(cpu)?,
        pipeline: Arc::new(RwLock::new(None)),
        version: crate::version::current(),
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

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port.unwrap_or(0))).await?;
    let api_port = listener.local_addr()?.port();
    let shared: SharedResources = Arc::new(tokio::sync::OnceCell::new());
    let mut context = tauri::generate_context!();
    let shared_assets = crate::assets::share_context_assets(&mut context);

    if headless {
        let resolver =
            server::asset_resolver([crate::assets::embedded_asset_resolver(shared_assets)]);
        tauri::async_runtime::spawn({
            let shared = shared.clone();
            async move {
                if let Err(err) = server::serve_with_listener(listener, shared, resolver).await {
                    tracing::error!("Server error: {err:#}");
                }
            }
        });
        shared
            .get_or_try_init(|| async { build_resources(cpu, headless).await })
            .await?;
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    let embedded_resolver = crate::assets::embedded_asset_resolver(shared_assets);
    tauri::Builder::default()
        .append_invoke_initialization_script(format!("window.__KOHARU_API_PORT__ = {api_port};"))
        .setup(move |app| {
            let resolver = server::asset_resolver([
                crate::assets::tauri_asset_resolver(app.asset_resolver()),
                embedded_resolver,
            ]);
            tauri::async_runtime::spawn({
                let shared = shared.clone();
                async move {
                    if let Err(err) = server::serve_with_listener(listener, shared, resolver).await
                    {
                        tracing::error!("Server error: {err:#}");
                    }
                }
            });

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                handle
                    .plugin(tauri_plugin_updater::Builder::new().build())
                    .ok();

                shared
                    .get_or_try_init(|| async { build_resources(cpu, headless).await })
                    .await
                    .expect("failed to build app resources");

                // Hidden webview still excutes JavaScript,
                // which will trigger the API calls when bootstrapping (not ready).
                // We manually create the webview ONLY after resources are ready.
                // ref: https://github.com/tauri-apps/tauri/issues/10950
                let main_config = handle
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|window| window.label == "main")
                    .cloned()
                    .expect("main window config not found");
                let main_window = WebviewWindowBuilder::from_config(&handle, &main_config)
                    .expect("failed to build main window builder")
                    .build()
                    .expect("failed to create main window");

                handle
                    .get_webview_window("splashscreen")
                    .expect("splashscreen window not found")
                    .close()
                    .ok();
                main_window.show().ok();
            });
            Ok(())
        })
        .run(context)?;

    Ok(())
}

