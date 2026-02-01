use tauri::State;

use crate::result::Result;

#[tauri::command]
pub async fn initialize(port: State<'_, u16>) -> Result<u16> {
    Ok(*port.inner())
}
