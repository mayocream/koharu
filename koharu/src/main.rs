// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let ui = AppWindow::new()?;

    ui.run()?;

    Ok(())
}
