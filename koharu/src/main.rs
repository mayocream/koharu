#![forbid(unsafe_code)]
// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use koharu::app;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Ok(app::run().await?)
}
