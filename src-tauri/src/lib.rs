use tokio::sync::Mutex;

use tauri::{AppHandle, Manager, async_runtime::spawn};

mod comic_text_detector;

#[derive(Default)]
struct AppState {
    ctd: Option<comic_text_detector::ComicTextDetector>,
}

async fn initialize(app: AppHandle) -> anyhow::Result<()> {
    let state = app.state::<Mutex<AppState>>();
    let mut state = state.lock().await;
    state.ctd = Some(comic_text_detector::ComicTextDetector::new()?);

    // hide the splash screen
    if let Some(splashscreen) = app.get_webview_window("splashscreen") {
        splashscreen.close()?;
    }

    // show the main window
    if let Some(window) = app.get_webview_window("main") {
        window.show()?;
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(AppState::default()))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_persisted_scope::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            spawn(initialize(app.handle().clone()));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
