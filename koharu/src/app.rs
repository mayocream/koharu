use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::Parser;
use koharu_ml::cuda_is_available;
use koharu_runtime::{ensure_dylibs, preload_dylibs};
use once_cell::sync::Lazy;
use rfd::MessageDialog;
use tauri::Manager;
use tokio::sync::RwLock;
use tracing_subscriber::fmt::format::FmtSpan;

use crate::{command, llm, ml, renderer::TextRenderer, state::State};

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

#[cfg(target_os = "windows")]
mod windows_ansi {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::ENABLE_VIRTUAL_TERMINAL_PROCESSING;
    use windows::Win32::System::Console::GetConsoleMode;
    use windows::Win32::System::Console::GetStdHandle;
    use windows::Win32::System::Console::STD_OUTPUT_HANDLE;
    use windows::Win32::System::Console::SetConsoleMode;
    use windows::core::Result;

    pub fn enable_ansi_support() -> Result<()> {
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE)?;
            if handle == HANDLE::default() {
                println!("Failed to get console handle");
                return Ok(());
            }

            let mut mode = std::mem::zeroed();
            GetConsoleMode(handle, &mut mode)?;
            SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING)?;
            Ok(())
        }
    }
}

static APP_ROOT: Lazy<PathBuf> = Lazy::new(resolve_app_root);
static LIB_ROOT: Lazy<PathBuf> = Lazy::new(|| APP_ROOT.join("libs"));
static MODEL_ROOT: Lazy<PathBuf> = Lazy::new(|| APP_ROOT.join("models"));

#[derive(Parser)]
#[command(version, about)]
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
}

fn initialize() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows_ansi::enable_ansi_support().ok();
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

    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Panic")
            .set_description(&msg)
            .show();
        std::process::exit(1);
    }));

    #[cfg(feature = "bundle")]
    {
        // https://docs.velopack.io/integrating/overview#application-startup
        velopack::VelopackApp::build().run();
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

async fn setup(app: tauri::AppHandle, use_cpu: bool) -> Result<()> {
    #[cfg(feature = "bundle")]
    {
        let source = velopack::sources::HttpSource::new(
            "https://github.com/mayocream/koharu/releases/latest/download",
        );
        let um = velopack::UpdateManager::new(source, None, None)?;
        if let velopack::UpdateCheck::UpdateAvailable(updates) = um.check_for_updates()? {
            um.download_updates(&updates, None)?;
            um.apply_updates_and_restart(&updates)?;
        }
    }

    // Preload dynamic libraries only if CUDA is available.
    if cuda_is_available() {
        ensure_dylibs(LIB_ROOT.to_path_buf()).await?;
        preload_dylibs(LIB_ROOT.to_path_buf())?;
    }

    let ml = Arc::new(ml::Model::new(use_cpu).await?);
    let llm = Arc::new(llm::Model::new(use_cpu));
    let renderer = Arc::new(TextRenderer::new());
    let state = Arc::new(RwLock::new(State::default()));

    app.manage(ml);
    app.manage(llm);
    app.manage(renderer);
    app.manage(state);

    app.get_webview_window("splashscreen").unwrap().close()?;
    app.get_webview_window("main").unwrap().show()?;

    Ok(())
}

pub async fn run() -> Result<()> {
    initialize()?;

    let cli = Cli::parse();
    if cli.download {
        prefetch().await?;
        return Ok(());
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            command::open_external,
            command::open_documents,
            command::save_document,
            command::save_all_documents,
            command::detect,
            command::ocr,
            command::inpaint,
            command::render,
            command::update_text_blocks,
            command::llm_list,
            command::llm_load,
            command::llm_offload,
            command::llm_ready,
            command::llm_generate,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = setup(handle, cli.cpu).await {
                    panic!("application setup failed: {err:#}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
