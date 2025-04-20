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

    app.get_webview_window("splashscreen").unwrap().close()?;
    app.get_webview_window("main").unwrap().show()?;

    Ok(())
}

#[tauri::command]
async fn detect(
    state: tauri::State<'_, AppState>,
    image: Vec<u8>,
) -> Result<comic_text_detector::Output, String> {
    let ctd = state
        .ctd
        .as_ref()
        .ok_or_else(|| "ComicTextDetector not initialized".to_string())?;

    let img = image::load_from_memory(&image).map_err(|e| e.to_string())?;
    let result = ctd.inference(&img, 0.5, 0.5).map_err(|e| e.to_string())?;

    Ok(result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_persisted_scope::init())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            spawn(initialize(app.handle().clone()));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![detect])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
