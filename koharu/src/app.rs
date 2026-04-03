use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::{net::TcpListener, sync::RwLock};

use koharu_app::{AppResources, config as app_config, engine, llm, storage::Storage};
use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_ml::{Device, device};
use koharu_rpc::{SharedState, server};
use koharu_runtime::{ComputePolicy, RuntimeManager};

#[derive(Parser)]
#[command(version = crate::version::APP_VERSION, about)]
struct Cli {
    #[arg(short, long, help = "Download dynamic libraries and exit")]
    download: bool,
    #[arg(long, help = "Force CPU even if GPU is available")]
    cpu: bool,
    #[arg(short, long, value_name = "PORT", help = "Bind to a specific port")]
    port: Option<u16>,
    #[arg(long, help = "Run without GUI")]
    headless: bool,
    #[arg(long, help = "Use env vars for API keys instead of keyring")]
    no_keyring: bool,
    #[arg(long, help = "Enable debug console output")]
    debug: bool,
}

async fn build_resources(
    runtime: RuntimeManager,
    data_root: camino::Utf8PathBuf,
    cpu: bool,
) -> Result<AppResources> {
    let cpu = matches!(device(cpu)?, Device::Cpu);

    runtime
        .prepare()
        .await
        .context("Failed to prepare runtime")?;

    #[cfg(target_os = "windows")]
    crate::windows::register_khr().ok();

    // FIXME: llama.cpp might not need when a external LLM provider is used, but currently it's required to initialize the safe backend
    koharu_llm::sys::initialize(&runtime).context("failed to init llama.cpp")?;
    let backend = Arc::new(LlamaBackend::init().context("failed to init llama backend")?);

    let llm = Arc::new(llm::Model::new(runtime.clone(), cpu, backend));
    let storage = Arc::new(Storage::open(data_root.as_std_path())?);
    let registry = Arc::new(engine::Registry::new());
    let config = app_config::load().unwrap_or_default();

    Ok(AppResources {
        runtime,
        storage,
        registry,
        config: Arc::new(RwLock::new(config)),
        llm,
        device: device(cpu)?,
        pipeline: Arc::new(RwLock::new(None)),
        version: crate::version::current(),
    })
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    // ── Platform & logging ───────────────────────────────────────────
    #[cfg(target_os = "windows")]
    {
        let attached = crate::windows::attach_parent_console();
        if !attached && (cli.headless || cli.debug) {
            crate::windows::create_console_window();
        }
        crate::windows::enable_ansi_support().ok();
    }

    tracing_subscriber::fmt()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .with_env_filter(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    if cli.headless {
        std::panic::set_hook(Box::new(|info| eprintln!("panic: {info}")));
    } else {
        std::panic::set_hook(Box::new(|info| {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Panic")
                .set_description(info.to_string())
                .show();
            std::process::exit(1);
        }));
    }

    if cli.no_keyring {
        koharu_llm::providers::disable_keyring();
    }

    // ── Config ───────────────────────────────────────────────────────
    let config = app_config::load()?;
    let data_root = config.data.path.clone();
    let compute = if cli.cpu {
        ComputePolicy::CpuOnly
    } else {
        ComputePolicy::PreferGpu
    };

    if cli.download {
        return RuntimeManager::new(data_root.as_std_path(), compute)?
            .prepare()
            .await
            .context("Failed to download runtime packages");
    }

    // ── Server ───────────────────────────────────────────────────────
    let runtime = RuntimeManager::new(data_root.as_std_path(), compute)?;
    let default_port = if cfg!(debug_assertions) { 9999 } else { 0 };
    let listener =
        TcpListener::bind(format!("127.0.0.1:{}", cli.port.unwrap_or(default_port))).await?;
    let port = listener.local_addr()?.port();
    let resources: Arc<tokio::sync::OnceCell<AppResources>> = Default::default();
    let shared = SharedState::new(Arc::clone(&resources), runtime.clone());
    let mut context = tauri::generate_context!();
    let assets = crate::assets::from_context(&mut context);

    tracing::info!(root = %runtime.root().display(), port, "starting server");

    if cli.headless {
        tauri::async_runtime::spawn(server::serve_with_listener(listener, shared, assets));
        resources
            .get_or_try_init(|| build_resources(runtime, data_root, cli.cpu))
            .await?;
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    // ── GUI ──────────────────────────────────────────────────────────
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            tauri::async_runtime::spawn(server::serve_with_listener(listener, shared, assets));

            tauri::async_runtime::spawn(async move {
                if let Err(err) = resources
                    .get_or_try_init(|| build_resources(runtime, data_root, cli.cpu))
                    .await
                {
                    tracing::error!("Failed to build resources: {err:#}");
                    std::process::exit(1);
                }
            });

            let url: tauri::Url = if cfg!(debug_assertions) {
                // Dev: use Next.js dev server (rewrites proxy API to Axum)
                app.config()
                    .build
                    .dev_url
                    .clone()
                    .expect("dev_url must be set in dev mode")
            } else {
                // Production: load from Axum server (same-origin for API)
                format!("http://127.0.0.1:{port}").parse()?
            };
            let wc = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .expect("main window config not found");
            tauri::webview::WebviewWindowBuilder::from_config(app, &wc)?
                .build()?
                .navigate(url)?;

            let handle = app.handle().clone();
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
