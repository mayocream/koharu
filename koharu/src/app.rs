use std::sync::Arc;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use rfd::MessageDialog;
use tauri::{AppHandle, Manager, WebviewWindowBuilder};
use tokio::{net::TcpListener, sync::RwLock};
use tracing_subscriber::fmt::format::FmtSpan;

use koharu_app::{
    AppResources,
    blob_store::BlobStore,
    config::{self as app_config},
    llm,
    manifest::ManifestStore,
    ml,
    page_cache::PageCache,
    renderer::Renderer,
};
use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_ml::{cuda_is_available, device};
use koharu_rpc::{SharedState, server};
use koharu_runtime::{ComputePolicy, DirectorySetting, RuntimeManager, Settings};

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
        help = "Disable keyring and read API keys from environment variables instead (e.g. KOHARU_OPENAI_API_KEY)",
        default_value_t = false
    )]
    no_keyring: bool,
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

async fn prefetch(cpu: bool) -> Result<()> {
    let config = app_config::load()?;
    let data_root = config.data.path.clone();
    RuntimeManager::new(
        Settings {
            runtime: DirectorySetting {
                path: data_root.join("runtime"),
            },
            models: DirectorySetting {
                path: data_root.join("models"),
            },
        },
        if cpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?
    .prepare()
    .await
    .context("Failed to initialize runtime and model packages")?;
    Ok(())
}

fn build_llama_backend(runtime: &RuntimeManager) -> Result<Arc<LlamaBackend>> {
    koharu_llm::sys::initialize(runtime)
        .context("failed to initialize llama.cpp runtime bindings")?;
    let backend = LlamaBackend::init().context("unable to initialize llama.cpp backend")?;
    Ok(Arc::new(backend))
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

async fn build_resources(
    runtime: RuntimeManager,
    data_root: Utf8PathBuf,
    cpu: bool,
    headless: bool,
) -> Result<AppResources> {
    let mut cpu = cpu;

    if !cpu && cuda_is_available() {
        match koharu_runtime::nvidia_driver_version() {
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

    runtime
        .prepare()
        .await
        .context("Failed to initialize runtime and model packages")?;

    if !cpu && cuda_is_available() {
        #[cfg(target_os = "windows")]
        {
            if let Err(err) = crate::windows::register_khr() {
                tracing::warn!(?err, "Failed to register .khr file association");
            }
        }

        tracing::info!("CUDA is available and runtime packages were initialized");
    }

    let llama_backend = build_llama_backend(&runtime)?;
    let ml = Arc::new(
        ml::Model::new(&runtime, cpu, Arc::clone(&llama_backend))
            .await
            .context("Failed to initialize ML model")?,
    );
    let llm = Arc::new(llm::Model::new(runtime.clone(), cpu, llama_backend));
    let renderer = Arc::new(Renderer::new().context("Failed to initialize renderer")?);

    let blobs = BlobStore::new(data_root.join("blobs"))?;
    let manifests = ManifestStore::new(data_root.join("pages"))?;
    let cache = PageCache::new(blobs, manifests);

    Ok(AppResources {
        runtime,
        cache,
        ml,
        llm,
        renderer,
        device: device(cpu)?,
        pipeline: Arc::new(RwLock::new(None)),
        version: crate::version::current(),
    })
}

async fn initialize_resources(
    resources: Arc<tokio::sync::OnceCell<AppResources>>,
    runtime: RuntimeManager,
    data_root: Utf8PathBuf,
    cpu: bool,
    headless: bool,
) -> Result<()> {
    if resources.get().is_some() {
        return Ok(());
    }

    let settings = runtime.settings();
    tracing::info!(
        runtime_root = %settings.runtime.path,
        models_root = %settings.models.path,
        "initializing application resources"
    );

    resources
        .get_or_try_init(|| {
            let runtime = runtime.clone();
            let data_root = data_root.clone();
            async move { build_resources(runtime, data_root, cpu, headless).await }
        })
        .await?;

    Ok(())
}

fn create_window(handle: &AppHandle, label: &str) -> Result<()> {
    if handle.get_webview_window(label).is_some() {
        return Ok(());
    }

    let window_config = handle
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == label)
        .cloned()
        .with_context(|| format!("window config `{label}` not found"))?;

    WebviewWindowBuilder::from_config(handle, &window_config)
        .with_context(|| format!("failed to build `{label}` window"))?
        .build()
        .with_context(|| format!("failed to create `{label}` window"))?;

    if let Some(window) = handle.get_webview_window(label) {
        window.show().ok();
    }

    Ok(())
}

fn show_main_window(handle: &AppHandle) -> Result<()> {
    create_window(handle, "main")?;
    if let Some(main_window) = handle.get_webview_window("main") {
        main_window.show().ok();
    }

    Ok(())
}

pub async fn run() -> Result<()> {
    let Cli {
        download,
        cpu,
        port,
        headless,
        no_keyring,
        debug,
    } = Cli::parse();

    if no_keyring {
        koharu_llm::providers::disable_keyring();
    }

    initialize(headless, debug)?;

    if download {
        prefetch(cpu).await?;
        return Ok(());
    }

    let config = app_config::load()?;
    let data_root = config.data.path.clone();
    let initial_runtime = RuntimeManager::new(
        Settings {
            runtime: DirectorySetting {
                path: data_root.join("runtime"),
            },
            models: DirectorySetting {
                path: data_root.join("models"),
            },
        },
        if cpu {
            ComputePolicy::CpuOnly
        } else {
            ComputePolicy::PreferGpu
        },
    )?;
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port.unwrap_or(0))).await?;
    let api_port = listener.local_addr()?.port();
    let resources = Arc::new(tokio::sync::OnceCell::new());
    let shared = SharedState::new(Arc::clone(&resources), initial_runtime.clone());
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
        initialize_resources(
            Arc::clone(&resources),
            initial_runtime.clone(),
            data_root.clone(),
            cpu,
            headless,
        )
        .await?;
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    let embedded_resolver = crate::assets::embedded_asset_resolver(shared_assets);
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
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
            show_main_window(&handle)?;
            tauri::async_runtime::spawn({
                let resources = Arc::clone(&resources);
                let runtime = initial_runtime.clone();
                let data_root = data_root.clone();
                async move {
                    initialize_resources(resources, runtime, data_root, cpu, headless)
                        .await
                        .expect("failed to build app resources");
                }
            });

            tauri::async_runtime::spawn(async move {
                handle
                    .plugin(tauri_plugin_updater::Builder::new().build())
                    .ok();
            });

            Ok(())
        })
        .run(context)?;

    Ok(())
}
