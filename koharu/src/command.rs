use std::sync::atomic::{AtomicU16, Ordering};

use tauri::State;

use crate::result::Result;

#[tauri::command]
pub async fn initialize(port: State<'_, AtomicU16>) -> Result<u16> {
    let mut attempts = 0;
    loop {
        let p = port.load(Ordering::SeqCst);
        if p != 0 {
            return Ok(p);
        }

        if attempts >= 100 {
            return Err(anyhow::anyhow!("HTTP server failed to start").into());
        }

        attempts += 1;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
