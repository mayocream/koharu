use std::sync::Arc;

use anyhow::Result;
use ort::execution_providers::ExecutionProvider;
use rfd::MessageDialog;
use tauri::Manager;
use tokio::sync::RwLock;

use crate::{command, llm, onnx, renderer::TextRenderer, state::State};

fn initialize() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

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

async fn setup(app: tauri::AppHandle) -> Result<()> {
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

    // Pre-download dynamic libraries
    {
        let lib_dir = dirs::data_local_dir()
            .unwrap_or_default()
            .join("Koharu")
            .join("libs");
        koharu_runtime::ensure_dylibs(&lib_dir).await?;
        koharu_runtime::preload_dylibs(&lib_dir)?;
    }
    // Initialize ONNX Runtime
    {
        let cuda = ort::execution_providers::CUDAExecutionProvider::default();
        let coreml = ort::execution_providers::CoreMLExecutionProvider::default();

        if cuda.is_available().map_err(anyhow::Error::from)? {
            tracing::info!("Using CUDA Execution Provider for ONNX Runtime.");
        } else if coreml.is_available().map_err(anyhow::Error::from)? {
            tracing::info!("Using CoreML Execution Provider for ONNX Runtime.");
        } else {
            tracing::info!("Using default CPU Execution Provider for ONNX Runtime.");
        }

        ort::init()
            .with_execution_providers([
                #[cfg(feature = "cuda")]
                cuda.build(),
                #[cfg(feature = "coreml")]
                coreml.build(),
            ])
            .commit()?;
    }

    let onnx = Arc::new(onnx::Model::new().await?);
    let llm = Arc::new(llm::Model::new());
    let renderer = Arc::new(TextRenderer::new());
    let state = Arc::new(RwLock::new(State::default()));

    app.manage(onnx);
    app.manage(llm);
    app.manage(renderer);
    app.manage(state);

    app.get_webview_window("splashscreen").unwrap().close()?;
    app.get_webview_window("main").unwrap().show()?;

    Ok(())
}

pub fn run() -> Result<()> {
    initialize()?;

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            command::open_external,
            command::open_documents,
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
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = setup(handle).await {
                    panic!("application setup failed: {err:#}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
