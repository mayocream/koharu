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
use serde::Serialize;
use tauri::{Emitter, Manager};
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

const STARTUP_PROGRESS_EVENT: &str = "startup:progress";
const STARTUP_ERROR_EVENT: &str = "startup:error";
const STARTUP_TOTAL_STEPS: u8 = 8;
const STEP_PREPARING: u8 = 1;
const STEP_RUNTIME: u8 = 2;
const STEP_COMIC_TEXT_DETECTOR: u8 = 3;
const STEP_MANGA_OCR: u8 = 4;
const STEP_LAMA: u8 = 5;
const STEP_FONT_DETECTOR: u8 = 6;
const STEP_RENDERER: u8 = 7;
const STEP_SERVER: u8 = 8;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupProgressPayload {
    stage: &'static str,
    current: u8,
    total: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupErrorPayload {
    code: &'static str,
}

#[derive(Clone)]
pub struct AppResources {
    pub state: AppState,
    pub ml: Arc<ml::Model>,
    pub llm: Arc<llm::Model>,
    pub renderer: Arc<Renderer>,
    pub ml_device: DeviceName,
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

fn emit_startup_progress(app: &tauri::AppHandle, payload: StartupProgressPayload) {
    if let Some(splashscreen) = app.get_webview_window("splashscreen")
        && let Err(err) = splashscreen.emit(STARTUP_PROGRESS_EVENT, payload)
    {
        tracing::debug!(?err, "Failed to emit startup progress");
    }
}

fn emit_startup_error(app: &tauri::AppHandle, code: &'static str) {
    if let Some(splashscreen) = app.get_webview_window("splashscreen")
        && let Err(err) = splashscreen.emit(STARTUP_ERROR_EVENT, StartupErrorPayload { code })
    {
        tracing::error!(?err, "Failed to emit startup error");
    }
}

#[derive(Debug, Clone, Copy)]
enum AppLocale {
    EnUs,
    ZhCn,
    ZhTw,
    JaJp,
}

fn detect_app_locale() -> AppLocale {
    let locale = sys_locale::get_locale()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if locale.starts_with("zh")
        && (locale.contains("tw")
            || locale.contains("hk")
            || locale.contains("mo")
            || locale.contains("hant"))
    {
        return AppLocale::ZhTw;
    }

    if locale.starts_with("zh") {
        return AppLocale::ZhCn;
    }

    if locale.starts_with("ja") {
        return AppLocale::JaJp;
    }

    AppLocale::EnUs
}

fn startup_error_title(locale: AppLocale) -> &'static str {
    match locale {
        AppLocale::EnUs => "Initialization Failed",
        AppLocale::ZhCn => "初始化失败",
        AppLocale::ZhTw => "初始化失敗",
        AppLocale::JaJp => "初期化に失敗しました",
    }
}

fn startup_error_message(locale: AppLocale, code: &'static str) -> &'static str {
    match locale {
        AppLocale::EnUs => match code {
            "network_hf_download" => "Network error: unable to download models from Hugging Face.",
            "hf_download" => "Failed to download required models from Hugging Face.",
            "ml_init" => "Failed to initialize ML models.",
            "runtime_init" => "Failed to initialize ML runtime dependencies.",
            "server_start" => "Failed to start local backend service.",
            _ => "Initialization failed due to an unknown error.",
        },
        AppLocale::ZhCn => match code {
            "network_hf_download" => "网络错误：无法从 Hugging Face 拉取模型。",
            "hf_download" => "无法从 Hugging Face 下载必需模型。",
            "ml_init" => "ML 模型初始化失败。",
            "runtime_init" => "ML 运行时依赖初始化失败。",
            "server_start" => "本地后端服务启动失败。",
            _ => "初始化失败：未知错误。",
        },
        AppLocale::ZhTw => match code {
            "network_hf_download" => "網路錯誤：無法從 Hugging Face 下載模型。",
            "hf_download" => "無法從 Hugging Face 下載必要模型。",
            "ml_init" => "ML 模型初始化失敗。",
            "runtime_init" => "ML 執行階段依賴初始化失敗。",
            "server_start" => "本機後端服務啟動失敗。",
            _ => "初始化失敗：未知錯誤。",
        },
        AppLocale::JaJp => match code {
            "network_hf_download" => {
                "ネットワークエラー: Hugging Face からモデルを取得できません。"
            }
            "hf_download" => "Hugging Face から必要なモデルをダウンロードできません。",
            "ml_init" => "ML モデルの初期化に失敗しました。",
            "runtime_init" => "ML ランタイム依存関係の初期化に失敗しました。",
            "server_start" => "ローカルバックエンドサービスの起動に失敗しました。",
            _ => "不明なエラーで初期化に失敗しました。",
        },
    }
}

fn show_startup_error_dialog(code: &'static str) {
    let locale = detect_app_locale();
    MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(startup_error_title(locale))
        .set_description(startup_error_message(locale, code))
        .show();
}

fn classify_startup_error(err: &anyhow::Error) -> &'static str {
    let message = format!("{err:#}").to_ascii_lowercase();

    let hf_download = message.contains("failed to download from hf hub")
        || message.contains("huggingface.co")
        || message.contains("resolve/main");
    let network_related = message.contains("request error")
        || message.contains("connect")
        || message.contains("connection")
        || message.contains("timed out")
        || message.contains("timeout")
        || message.contains("dns")
        || message.contains("network")
        || message.contains("socket");

    if hf_download && network_related {
        return "network_hf_download";
    }
    if hf_download {
        return "hf_download";
    }
    if message.contains("failed to initialize ml model") {
        return "ml_init";
    }
    if message.contains("failed to ensure dynamic libraries")
        || message.contains("failed to preload dynamic libraries")
        || message.contains("failed to add dll directory")
    {
        return "runtime_init";
    }
    if message.contains("failed to bind http server") || message.contains("address already in use")
    {
        return "server_start";
    }

    "unknown"
}

fn model_stage_progress(stage: ml::InitStage) -> StartupProgressPayload {
    match stage {
        ml::InitStage::ComicTextDetector => StartupProgressPayload {
            stage: "loadingComicTextDetector",
            current: STEP_COMIC_TEXT_DETECTOR,
            total: STARTUP_TOTAL_STEPS,
        },
        ml::InitStage::MangaOcr => StartupProgressPayload {
            stage: "loadingMangaOcr",
            current: STEP_MANGA_OCR,
            total: STARTUP_TOTAL_STEPS,
        },
        ml::InitStage::Lama => StartupProgressPayload {
            stage: "loadingLama",
            current: STEP_LAMA,
            total: STARTUP_TOTAL_STEPS,
        },
        ml::InitStage::FontDetector => StartupProgressPayload {
            stage: "loadingFontDetector",
            current: STEP_FONT_DETECTOR,
            total: STARTUP_TOTAL_STEPS,
        },
    }
}

async fn build_resources<F>(
    cpu: bool,
    _register_file_assoc: bool,
    mut report: F,
) -> Result<AppResources>
where
    F: FnMut(StartupProgressPayload),
{
    report(StartupProgressPayload {
        stage: "checkingRuntime",
        current: STEP_RUNTIME,
        total: STARTUP_TOTAL_STEPS,
    });

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
        ml::Model::new_with_progress(cpu, |stage| {
            report(model_stage_progress(stage));
        })
        .await
        .context("Failed to initialize ML model")?,
    );
    let llm = Arc::new(llm::Model::new(cpu));
    report(StartupProgressPayload {
        stage: "loadingRenderer",
        current: STEP_RENDERER,
        total: STARTUP_TOTAL_STEPS,
    });
    let renderer = Arc::new(Renderer::new().context("Failed to initialize renderer")?);
    let state = Arc::new(RwLock::new(State::default()));

    Ok(AppResources {
        state,
        ml,
        llm,
        renderer,
        ml_device,
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
        let resources = build_resources(cpu, false, |_| {}).await?;
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
        .invoke_handler(tauri::generate_handler![command::initialize])
        .manage(AtomicU16::new(0))
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                emit_startup_progress(
                    &handle,
                    StartupProgressPayload {
                        stage: "preparing",
                        current: STEP_PREPARING,
                        total: STARTUP_TOTAL_STEPS,
                    },
                );

                let progress_handle = handle.clone();
                let resources = match build_resources(cpu, true, move |payload| {
                    emit_startup_progress(&progress_handle, payload);
                })
                .await
                {
                    Ok(resources) => resources,
                    Err(err) => {
                        let code = classify_startup_error(&err);
                        tracing::error!(?err, code, "Failed to build app resources");
                        emit_startup_error(&handle, code);
                        show_startup_error_dialog(code);
                        return;
                    }
                };

                emit_startup_progress(
                    &handle,
                    StartupProgressPayload {
                        stage: "startingServer",
                        current: STEP_SERVER,
                        total: STARTUP_TOTAL_STEPS,
                    },
                );

                let listener = match TcpListener::bind(format!("127.0.0.1:{}", port.unwrap_or(0)))
                    .await
                {
                    Ok(listener) => listener,
                    Err(err) => {
                        let err = anyhow::Error::from(err).context("Failed to bind HTTP server");
                        let code = classify_startup_error(&err);
                        tracing::error!(?err, code, "Failed to start HTTP server");
                        emit_startup_error(&handle, code);
                        show_startup_error_dialog(code);
                        return;
                    }
                };

                let port = match listener.local_addr() {
                    Ok(addr) => addr.port(),
                    Err(err) => {
                        let err = anyhow::Error::from(err)
                            .context("Failed to get HTTP server listener address");
                        tracing::error!(?err, "Failed to read listener address");
                        emit_startup_error(&handle, "server_start");
                        show_startup_error_dialog("server_start");
                        return;
                    }
                };

                handle.state::<AtomicU16>().store(port, Ordering::SeqCst);

                let server_resources = resources.clone();
                tokio::spawn(async move {
                    if let Err(err) = server::serve_with_listener(listener, server_resources).await
                    {
                        tracing::error!("HTTP server error: {err:#}");
                    }
                });

                if let Some(splashscreen) = handle.get_webview_window("splashscreen") {
                    splashscreen.close().ok();
                }
                if let Some(main) = handle.get_webview_window("main") {
                    main.show().ok();
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
