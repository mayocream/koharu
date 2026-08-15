#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser as _;
use koharu::panic;
use koharu::sentry;
use koharu_app as app;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

#[derive(clap::Parser)]
#[command(version, about)]
struct Cli {}

#[tokio::main]
#[tauri::cef_entry_point]
async fn main() {
    #[cfg(target_os = "windows")]
    {
        // SAFETY: This only requests the existing parent console. It does not allocate one.
        let _ = unsafe {
            windows::Win32::System::Console::AttachConsole(
                windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
            )
        };
    }

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
    tokio::task::block_in_place(|| app::run(tauri::generate_context!()))
        .expect("failed to run the desktop application");
}
