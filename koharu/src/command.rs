use crate::result::Result;

#[tauri::command]
pub fn open_external(url: &str) -> Result<()> {
    open::that(url)?;

    Ok(())
}
