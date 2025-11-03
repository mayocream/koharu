use std::sync::Arc;

use anyhow::Result;
use rfd::MessageDialog;
use tauri::Manager;
use tokio::sync::RwLock;

use crate::command;
use crate::inference::Inference;
use crate::state::State;

fn initialize() -> Result<()> {
    tracing_subscriber::fmt().init();

    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Panic")
            .set_description(&msg)
            .show();
    }));

    #[cfg(feature = "bundle")]
    {
        // https://docs.velopack.io/integrating/overview#application-startup
        velopack::VelopackApp::build().run();
    }

    ort::init()
        .with_execution_providers([
            #[cfg(feature = "cuda")]
            ort::execution_providers::CUDAExecutionProvider::default()
                .build()
                .error_on_failure(),
        ])
        .commit()?;

    Ok(())
}

async fn setup(app: tauri::AppHandle) -> Result<()> {
    let inference = Arc::new(Inference::new()?);
    let state = Arc::new(RwLock::new(State::default()));

    app.manage(inference);
    app.manage(state);

    app.get_webview_window("splashscreen").unwrap().close()?;
    app.get_webview_window("main").unwrap().show()?;

    Ok(())
}

pub fn run() -> Result<()> {
    initialize()?;

    tauri::Builder::default()
        .setup(|app| {
            tauri::async_runtime::spawn(setup(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            command::open_external,
            command::open_documents,
            command::detect,
            command::ocr,
            command::inpaint,
        ])
        .run(tauri::generate_context!())?;

    Ok(())
}
