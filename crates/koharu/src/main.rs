#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser as _;
use koharu::panic;
use koharu::sentry;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

#[derive(clap::Parser)]
#[command(version, about)]
struct Cli {}

#[tauri::cef_entry_point]
#[tokio::main]
async fn main() {
    let _cli = Cli::parse();
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
    tokio::task::block_in_place(|| koharu::run(tauri::generate_context!()))
        .expect("failed to run the desktop application");
}
