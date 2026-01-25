use tauri::State;

use crate::{app::HttpServerState, result::Result};

#[tauri::command]
pub async fn initialize(http_state: State<'_, HttpServerState>) -> Result<u16> {
    // Wait for the HTTP server to be ready (port set)
    let mut attempts = 0;
    loop {
        let port = http_state.port();
        if port != 0 {
            return Ok(port);
        }

        if attempts >= 100 {
            return Err(anyhow::anyhow!("HTTP server failed to start").into());
        }

        attempts += 1;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
