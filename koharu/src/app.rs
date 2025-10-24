use anyhow::Result;
use ort::execution_providers::CUDAExecutionProvider;
use rfd::MessageDialog;
use slint::ComponentHandle;
use std::sync::Arc;

use crate::inference::Inference;
use crate::ui::{self, App};

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

    ui::setup(&app, inference);
    app.run()?;

    Ok(())
}
