use std::sync::Arc;

use anyhow::Result;
use ort::execution_providers::CUDAExecutionProvider;
use rfd::MessageDialog;
use slint::ComponentHandle;

use crate::callback;
use crate::inference::Inference;
use crate::ui::App;

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

    ort::init()
        .with_execution_providers([CUDAExecutionProvider::default().build().error_on_failure()])
        .commit()?;

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
