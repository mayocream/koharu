use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, sync::RwLock};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use koharu_app::{AppResources, config as app_config, engine, llm, storage::Storage};
use koharu_core::DocumentSummary;
use koharu_llm::ModelId;
use koharu_llm::safe::llama_backend::LlamaBackend;
use koharu_ml::{Device, device};
use koharu_rpc::{SharedState, server};
use koharu_runtime::{ComputePolicy, RuntimeHttpConfig, RuntimeManager};

use crate::chinese::ChineseVariant;

/// Result of importing from URL
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportUrlResult {
    pub total_count: usize,
    pub documents: Vec<DocumentSummary>,
}

/// Result of Chinese text conversion
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertChineseResult {
    pub text: String,
    pub from_variant: String,
    pub to_variant: String,
}

/// Tauri command to convert Chinese text between variants
#[tauri::command]
fn convert_chinese(
    text: String,
    to_variant: ChineseVariant,
) -> Result<ConvertChineseResult, String> {
    // Assume input is Simplified Chinese (from manhuagui.com sources)
    let converted = crate::chinese::convert_from_simplified(&text, to_variant)
        .map_err(|e| e.to_string())?;

    Ok(ConvertChineseResult {
        text: converted,
        from_variant: ChineseVariant::Simplified.display_name().to_string(),
        to_variant: to_variant.display_name().to_string(),
    })
}

/// Tauri command to get available Chinese variants
#[tauri::command]
fn get_chinese_variants() -> Vec<(String, String)> {
    vec![
        ("none".to_string(), ChineseVariant::None.display_name().to_string()),
        ("simplified".to_string(), ChineseVariant::Simplified.display_name().to_string()),
        ("traditional".to_string(), ChineseVariant::Traditional.display_name().to_string()),
        ("traditional_tw".to_string(), ChineseVariant::TraditionalTw.display_name().to_string()),
        ("traditional_twp".to_string(), ChineseVariant::TraditionalTwp.display_name().to_string()),
        ("traditional_hk".to_string(), ChineseVariant::TraditionalHk.display_name().to_string()),
    ]
}

/// Tauri command to import manga pages from a URL
#[tauri::command]
async fn import_from_url(
    url: String,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, SharedState>,
) -> Result<ImportUrlResult, String> {
    import_from_url_impl(&url, &app_handle, &state)
        .await
        .map_err(|e| e.to_string())
}

async fn import_from_url_impl(
    url: &str,
    app_handle: &tauri::AppHandle,
    state: &SharedState,
) -> anyhow::Result<ImportUrlResult> {
    // Scrape images from the URL
    let files = crate::scraper::scrape_manhuagui(url, app_handle).await?;

    if files.is_empty() {
        anyhow::bail!("No images were downloaded");
    }

    // Get resources to access storage
    let resources = state
        .get()
        .ok_or_else(|| anyhow::anyhow!("App resources not initialized"))?;

    // Import the files into storage (replace mode)
    let _imported = resources.storage.import_files(files, true).await?;

    // Return the list of documents
    let documents = resources.storage.list_pages().await;

    Ok(ImportUrlResult {
        total_count: documents.len(),
        documents,
    })
}

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
    runtime
        .prepare()
        .await
        .context("Failed to prepare runtime")?;

    let selected_device = device(cpu)?;
    let cpu = matches!(&selected_device, Device::Cpu);

    #[cfg(target_os = "windows")]
    crate::windows::register_khr().ok();

    // FIXME: llama.cpp might not need when a external LLM provider is used, but currently it's required to initialize the safe backend
    koharu_llm::sys::initialize(&runtime).context("failed to init llama.cpp")?;
    let backend = Arc::new(LlamaBackend::init().context("failed to init llama backend")?);
    koharu_llm::suppress_native_logs();

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
        device: selected_device,
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

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .with(crate::tracing_fmt::TimingLayer::new())
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
    let http = RuntimeHttpConfig {
        connect_timeout_secs: config.http.connect_timeout.max(1),
        read_timeout_secs: config.http.read_timeout.max(1),
        max_retries: config.http.max_retries,
    };
    let compute = if cli.cpu {
        ComputePolicy::CpuOnly
    } else {
        ComputePolicy::PreferGpu
    };

    if cli.download {
        return RuntimeManager::new_with_http(data_root.as_std_path(), compute, http.clone())?
            .prepare()
            .await
            .context("Failed to download runtime packages");
    }

    // ── Server ───────────────────────────────────────────────────────
    let runtime = RuntimeManager::new_with_http(data_root.as_std_path(), compute, http)?;
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
        let res = resources
            .get_or_try_init(|| build_resources(runtime, data_root, cli.cpu))
            .await?;
        // Auto-load Qwen3-8B model on startup
        tracing::info!("Auto-loading Qwen3-8B model");
        res.llm.load_local(ModelId::Qwen3_8b).await;
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    // ── GUI ──────────────────────────────────────────────────────────
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![import_from_url, convert_chinese, get_chinese_variants])
        .setup(move |app| {
            tauri::async_runtime::spawn(server::serve_with_listener(listener, shared, assets));

            tauri::async_runtime::spawn(async move {
                match resources
                    .get_or_try_init(|| build_resources(runtime, data_root, cli.cpu))
                    .await
                {
                    Ok(res) => {
                        // Auto-load Qwen3-8B model on startup
                        tracing::info!("Auto-loading Qwen3-8B model");
                        res.llm.load_local(ModelId::Qwen3_8b).await;
                    }
                    Err(err) => {
                        tracing::error!("Failed to build resources: {err:#}");
                        std::process::exit(1);
                    }
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
