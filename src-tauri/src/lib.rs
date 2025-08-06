use comic_text_detector::ComicTextDetector;
use lama::Lama;
use manga_ocr::MangaOCR;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::RwLock;

use tauri::{AppHandle, Manager, async_runtime::spawn};

#[derive(Debug)]
struct AppState {
    comic_text_detector: ComicTextDetector,
    manga_ocr: MangaOCR,
    lama: Lama,
}

// Initialize models
async fn initialize(app: AppHandle) -> anyhow::Result<()> {
    let comic_text_detector = ComicTextDetector::new()?;
    let manga_ocr = MangaOCR::new()?;
    let lama = Lama::new()?;

    app.manage(RwLock::new(AppState {
        comic_text_detector,
        manga_ocr,
        lama,
    }));

    app.get_webview_window("splashscreen").unwrap().close()?;
    app.get_webview_window("main").unwrap().show()?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> anyhow::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // initialize the app state
            let app_handle = app.handle().clone();
            spawn({
                async move {
                    if let Err(e) = initialize(app_handle.clone()).await {
                        app_handle
                            .notification()
                            .builder()
                            .title("Error")
                            .body(format!("Failed to initialize: {}", e))
                            .show()
                            .unwrap();
                        app_handle.exit(1);
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
