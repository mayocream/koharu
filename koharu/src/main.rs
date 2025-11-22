// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use koharu::app;

fn main() -> anyhow::Result<()> {
    Ok(app::run()?)
}
