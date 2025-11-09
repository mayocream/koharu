// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use koharu_app::app;

fn main() -> anyhow::Result<()> {
    Ok(app::run()?)
}
