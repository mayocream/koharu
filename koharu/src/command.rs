use std::sync::atomic::{AtomicU16, Ordering};

use tauri::State;
use tauri::ipc::Channel;

use crate::result::Result;

#[tauri::command]
pub async fn initialize(port: State<'_, AtomicU16>) -> Result<u16> {
    loop {
        let p = port.load(Ordering::SeqCst);
        if p != 0 {
            return Ok(p);
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tauri::command]
pub async fn download_progress(
    channel: Channel<koharu_core::download::DownloadProgress>,
) -> Result<()> {
    let mut rx = koharu_core::download::subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(progress) => {
                    if channel.send(progress).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}
