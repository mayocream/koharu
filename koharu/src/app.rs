use std::sync::Arc;

use anyhow::Result;
use rfd::MessageDialog;
use tauri::Manager;
use velopack::{UpdateCheck, UpdateManager};

use crate::inference::Inference;
use crate::update::GithubSource;

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
        // Check for updates at startup
        update()?;
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

#[allow(dead_code)]
fn update() -> Result<()> {
    let source = GithubSource::new("mayocream", "koharu");
    let update_manager = UpdateManager::new(source, None, None)?;

    if let UpdateCheck::UpdateAvailable(updates) = update_manager.check_for_updates()? {
        update_manager.download_updates(&updates, None)?;
        update_manager.apply_updates_and_restart(&updates)?;
    }

    Ok(())
}

async fn setup(app: tauri::AppHandle) -> Result<()> {
    let _inference = Arc::new(Inference::new()?);

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
        .invoke_handler(tauri::generate_handler![])
        .run(tauri::generate_context!())?;

    Ok(())
}
