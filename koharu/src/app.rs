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

use crate::{
    api, command, llm, ml,
    renderer::Renderer,
    state::{AppState, State},
    update,
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
}

#[derive(Parser)]
#[command(version = crate::version::APP_VERSION, about)]
struct Cli {
    #[arg(
        long,
        help = "Force using CPU even if GPU is available",
        default_value_t = false
    )]
    cpu: bool,
    #[arg(
        short = 'b',
        long = "bind",
        value_name = "BIND",
        help = "Run in headless mode and bind the HTTP server to this address, e.g. 127.0.0.1:23333"
    )]
    bind: Option<String>,
    #[arg(
        long,
        help = "Enable debug mode with console output",
        default_value_t = false
    )]
    debug: bool,
}

fn initialize(headless: bool, _debug_flag: bool) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        // hide console window in release mode and not headless
        if headless || _debug_flag {
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

async fn build_resources(use_cpu: bool, _register_file_assoc: bool) -> Result<AppResources> {
    if cuda_is_available() {
        ensure_dylibs(LIB_ROOT.to_path_buf()).await?;
        preload_dylibs(LIB_ROOT.to_path_buf())?;

        #[cfg(target_os = "windows")]
        {
            if _register_file_assoc && let Err(err) = crate::windows::register_khr() {
                tracing::warn!(?err, "Failed to register .khr file association");
            }

            crate::windows::add_dll_directory(&LIB_ROOT)?;
        }

        tracing::info!(
            "CUDA is available, loaded dynamic libraries from {:?}",
            *LIB_ROOT
        );
    }

    let ml = Arc::new(ml::Model::new(use_cpu).await?);
    let llm = Arc::new(llm::Model::new(use_cpu));
    let renderer = Arc::new(Renderer::new()?);
    let state = Arc::new(RwLock::new(State::default()));

    Ok(AppResources {
        state,
        ml,
        llm,
        renderer,
    })
}

async fn setup(app: tauri::AppHandle, cpu: bool) -> Result<()> {
    let resources = build_resources(cpu, true).await?;
    let state = resources.state.clone();

    app.manage(resources.ml);
    app.manage(resources.llm);
    app.manage(resources.renderer);

    app.get_webview_window("splashscreen").unwrap().close()?;
    app.get_webview_window("main").unwrap().show()?;

    app.manage(state);

    Ok(())
}

pub async fn run() -> Result<()> {
    let Cli { cpu, bind, debug } = Cli::parse();

    initialize(bind.is_some(), debug)?;

    if let Some(bind_addr) = bind {
        let resources = build_resources(cpu, false).await?;

        api::serve(bind_addr, resources).await?;
        return Ok(());
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            command::app_version,
            command::open_external,
            command::get_documents,
            command::open_documents,
            command::save_documents,
            command::export_document,
            command::export_all_documents,
            command::detect,
            command::ocr,
            command::inpaint,
            command::inpaint_partial,
            command::render,
            command::update_brush_layer,
            command::update_text_blocks,
            command::update_inpaint_mask,
            command::list_font_families,
            command::llm_list,
            command::llm_load,
            command::llm_offload,
            command::llm_ready,
            command::llm_generate,
            update::apply_available_update,
            update::get_available_update,
            update::ignore_update,
        ])
        .setup(move |app| {
            app.manage(update::UpdateState::new(APP_ROOT.to_path_buf()));
            update::spawn_background_update_check(app.handle().clone());

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = setup(handle, cpu).await {
                    panic!("application setup failed: {err:#}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
