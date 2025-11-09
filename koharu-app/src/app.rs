use std::sync::{Arc, Mutex};

use koharu_core::{result::Result, state::State};
use ort::execution_providers::ExecutionProvider;
use rfd::MessageDialog;
use tauri::Manager;
use tokio::sync::RwLock;
use tracing::{error, warn};

use crate::{command, llm, onnx, telemetry};

fn initialize() -> anyhow::Result<()> {
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

async fn setup(app: tauri::AppHandle) -> anyhow::Result<()> {
    telemetry::init(Arc::new(Mutex::new(app.clone())))?;

    // Dynamically dylibs depending on features automatically
    {
        let lib_root = dirs::data_local_dir()
            .ok_or_else(|| anyhow::anyhow!("Failed to get local data directory"))?
            .join("koharu")
            .join("lib");
        koharu_runtime::dylib::ensure_dylibs(lib_root.clone()).await?;
        koharu_runtime::dylib::preload_dylibs(&lib_root)?;
    }

    // Initialize ONNX Runtime
    {
        let cuda = ort::execution_providers::CUDAExecutionProvider::default();
        if !cuda.is_available().map_err(anyhow::Error::from)? {
            warn!(
                "CUDA Execution Provider is not available. Falling back to CPU Execution Provider."
            );
        }

        ort::init()
            .with_execution_providers([
                #[cfg(feature = "cuda")]
                cuda.build(),
            ])
            .commit()?;
    }

    let onnx = Arc::new(onnx::Model::new().await?);
    let llm = Arc::new(llm::Model::new());
    let state = Arc::new(RwLock::new(State::default()));

    app.manage(onnx);
    app.manage(llm);
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
                    error!("application setup failed: {err:#}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
