use std::sync::{Arc, Mutex as StdMutex};

use anyhow::{Context, Result};
use clap::Parser;
use rfd::MessageDialog;
use tauri::{AppHandle, Manager, WebviewWindowBuilder};
use tokio::{
    net::TcpListener,
    sync::{RwLock, watch},
};
use tracing_subscriber::fmt::format::FmtSpan;

use koharu_app::{
    AppResources,
    config::{self as app_config, AppConfig},
    llm, ml,
    renderer::Renderer,
};
use koharu_core::{BootstrapConfig, State};
use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_ml::{cuda_is_available, device};
use koharu_rpc::{BootstrapHooks, SharedState, server};
use koharu_runtime::{ComputePolicy, RuntimeManager};

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

#[derive(Clone)]
struct BootstrapController {
    resources: Arc<tokio::sync::OnceCell<AppResources>>,
    runtime_tx: watch::Sender<RuntimeManager>,
    init_lock: Arc<tokio::sync::Mutex<()>>,
    handle: Arc<StdMutex<Option<AppHandle>>>,
    cpu: bool,
    headless: bool,
}

impl BootstrapController {
    fn get_config(&self) -> Result<BootstrapConfig> {
        Ok(app_config::to_bootstrap_config(&app_config::load()?))
    }

    fn put_config(&self, config: BootstrapConfig) -> Result<BootstrapConfig> {
        let config = app_config::from_bootstrap_config(config)?;
        app_config::save(&config)?;
        if self.resources.get().is_none() {
            self.runtime_tx
                .send_replace(runtime_from_config(config.clone(), self.cpu)?);
        }
        Ok(app_config::to_bootstrap_config(&config))
    }

    fn set_handle(&self, handle: AppHandle) {
        if let Ok(mut slot) = self.handle.lock() {
            *slot = Some(handle);
        }
    }

    async fn initialize(&self) -> Result<()> {
        if self.resources.get().is_some() {
            if let Some(handle) = self.current_handle() {
                show_main_window(&handle)?;
            }
            return Ok(());
        }

        let _guard = self
            .init_lock
            .try_lock()
            .map_err(|_| anyhow::anyhow!("initialization already in progress"))?;

        let runtime = self.runtime_tx.borrow().clone();
        let settings = runtime.settings();
        tracing::info!(
            runtime_root = %settings.runtime.path.display(),
            models_root = %settings.models.path.display(),
            proxy = settings.http.proxy.as_ref().map(|value| value.as_str()).unwrap_or("<direct>"),
            "initializing application resources"
        );

        self.resources
            .get_or_try_init(|| {
                let runtime = runtime.clone();
                async move { build_resources(runtime, self.cpu, self.headless).await }
            })
            .await?;

        if let Some(handle) = self.current_handle() {
            show_main_window(&handle)?;
        }

        Ok(())
    }

    fn current_handle(&self) -> Option<AppHandle> {
        self.handle.lock().ok().and_then(|slot| slot.clone())
    }
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

fn compute_policy(cpu: bool) -> ComputePolicy {
    if cpu {
        ComputePolicy::CpuOnly
    } else {
        ComputePolicy::PreferGpu
    }
}

fn runtime_from_config(config: AppConfig, cpu: bool) -> Result<RuntimeManager> {
    RuntimeManager::new(config, compute_policy(cpu))
}

fn load_runtime(cpu: bool) -> Result<RuntimeManager> {
    runtime_from_config(app_config::load()?, cpu)
}

async fn prefetch(cpu: bool) -> Result<()> {
    load_runtime(cpu)?
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
    let state = Arc::new(RwLock::new(State::default()));

    Ok(AppResources {
        runtime,
        state,
        ml,
        llm,
        renderer,
        device: device(cpu)?,
        pipeline: Arc::new(RwLock::new(None)),
        version: crate::version::current(),
    })
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

    for label in ["splashscreen", "bootstrap"] {
        if let Some(window) = handle.get_webview_window(label) {
            window.close().ok();
        }
    }

    Ok(())
}

fn make_bootstrap_hooks(controller: Arc<BootstrapController>) -> BootstrapHooks {
    BootstrapHooks {
        get_config: Arc::new({
            let controller = Arc::clone(&controller);
            move || controller.get_config()
        }),
        put_config: Arc::new({
            let controller = Arc::clone(&controller);
            move |config| controller.put_config(config)
        }),
        initialize: Arc::new({
            let controller = Arc::clone(&controller);
            move || {
                let controller = Arc::clone(&controller);
                Box::pin(async move { controller.initialize().await })
            }
        }),
    }
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

    let initial_runtime = load_runtime(cpu)?;
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port.unwrap_or(0))).await?;
    let api_port = listener.local_addr()?.port();
    let resources = Arc::new(tokio::sync::OnceCell::new());
    let (runtime_tx, runtime_rx) = watch::channel(initial_runtime.clone());
    let controller = Arc::new(BootstrapController {
        resources: Arc::clone(&resources),
        runtime_tx,
        init_lock: Arc::new(tokio::sync::Mutex::new(())),
        handle: Arc::new(StdMutex::new(None)),
        cpu,
        headless,
    });
    let shared = SharedState::new(
        Arc::clone(&resources),
        runtime_rx,
        make_bootstrap_hooks(controller.clone()),
    );
    let mut context = tauri::generate_context!();
    let shared_assets = crate::assets::share_context_assets(&mut context);

    if headless {
        let resolver = server::asset_resolver([
            crate::assets::filesystem_asset_resolver(),
            crate::assets::embedded_asset_resolver(shared_assets),
        ]);
        tauri::async_runtime::spawn({
            let shared = shared.clone();
            async move {
                if let Err(err) = server::serve_with_listener(listener, shared, resolver).await {
                    tracing::error!("Server error: {err:#}");
                }
            }
        });
        shared
            .get_or_try_init(|| {
                let runtime = initial_runtime.clone();
                async move { build_resources(runtime, cpu, headless).await }
            })
            .await?;
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    let embedded_resolver = crate::assets::embedded_asset_resolver(shared_assets);
    tauri::Builder::default()
        .append_invoke_initialization_script(format!("window.__KOHARU_API_PORT__ = {api_port};"))
        .setup(move |app| {
            let resolver = server::asset_resolver([
                crate::assets::filesystem_asset_resolver(),
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
            controller.set_handle(handle.clone());

            let runtime = initial_runtime.clone();
            let needs_bootstrap = runtime.needs_bootstrap().with_context(|| {
                format!(
                    "failed to inspect bootstrap packages for runtime `{}` and models `{}`",
                    runtime.settings().runtime.path.display(),
                    runtime.settings().models.path.display()
                )
            })?;

            if needs_bootstrap {
                create_window(&handle, "bootstrap")?;
            } else {
                create_window(&handle, "splashscreen")?;
                tauri::async_runtime::spawn({
                    let controller = controller.clone();
                    async move {
                        controller
                            .initialize()
                            .await
                            .expect("failed to build app resources");
                    }
                });
            }

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
