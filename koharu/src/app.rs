use std::sync::Arc;

use anyhow::Result;
use ort::execution_providers::CUDAExecutionProvider;
use rfd::MessageDialog;
use slint::ComponentHandle;
use velopack::{UpdateCheck, UpdateManager};

use crate::callback;
use crate::inference::Inference;
use crate::ui::App;
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

    #[cfg(not(debug_assertions))]
    {
        // https://docs.velopack.io/integrating/overview#application-startup
        velopack::VelopackApp::build().run();
        // Check for updates at startup
        update()?;
    }

    ort::init()
        .with_execution_providers([CUDAExecutionProvider::default().build().error_on_failure()])
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

pub fn run() -> Result<()> {
    initialize()?;

    let inference = Arc::new(Inference::new()?);
    let app = App::new()?;

    callback::setup(&app, inference);
    app.run()?;

    Ok(())
}
