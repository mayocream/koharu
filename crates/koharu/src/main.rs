#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use koharu::panic;
use koharu::sentry;
use std::process::ExitCode;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

fn main() -> ExitCode {
    if let Some(exit) = koharu_desktop::browser::dispatch_cef_process() {
        return exit;
    }

    let _guard = sentry::initialize();
    panic::install();
    let filter = tracing_subscriber::filter::EnvFilter::builder()
        .with_default_directive(tracing::Level::INFO.into())
        .from_env_lossy();
    tracing_subscriber::registry()
        .with(filter)
        .with(sentry::tracing_layer())
        .with(koharu::tracing::TimingLayer::new())
        .init();

    match application() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Koharu stopped: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[tokio::main]
async fn application() -> anyhow::Result<()> {
    koharu::run().await
}
