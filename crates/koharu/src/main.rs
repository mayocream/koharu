#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use koharu::app;
use koharu::panic;
use koharu::sentry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _guard = sentry::initialize();
    panic::install();
    app::run().await
}
