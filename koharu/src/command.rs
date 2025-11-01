use std::path::PathBuf;

use tauri::ipc;

use crate::result::Result;

#[tauri::command]
pub fn open_external(url: &str) -> Result<()> {
    open::that(url)?;

    Ok(())
}

#[tauri::command]
pub fn pick_files() -> Result<Vec<PathBuf>> {
    let paths = rfd::FileDialog::new()
        .set_title("Select Files")
        .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
        .pick_files()
        .unwrap_or_default();

    Ok(paths)
}

#[tauri::command]
pub fn read_file(path: &str) -> ipc::Response {
    let data = std::fs::read(path).unwrap();
    ipc::Response::new(data)
}
