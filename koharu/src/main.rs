#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use koharu::run;

#[tokio::main]
async fn main() -> miette::Result<()> {
    run().await.map_err(|error| miette::miette!("{error:?}"))
}
