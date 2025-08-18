mod commands;
mod error;

use comic_text_detector::ComicTextDetector;
use lama::Lama;
use manga_ocr::MangaOCR;
use tauri::{AppHandle, Manager, async_runtime::spawn};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tokio::sync::RwLock;

use crate::commands::{detection, inpaint, ocr};

#[derive(Debug)]
struct AppState {
    comic_text_detector: ComicTextDetector,
    manga_ocr: MangaOCR,
    lama: Lama,
}

// Initialize models
async fn initialize(app: AppHandle) -> anyhow::Result<()> {
    // refer: https://ort.pyke.io/perf/execution-providers#global-defaults
    ort::init()
        .with_execution_providers([
            #[cfg(feature = "cuda")]
            ort::execution_providers::CUDAExecutionProvider::default()
                .build()
                .error_on_failure(),
            #[cfg(not(feature = "cuda"))]
            ort::execution_providers::CPUExecutionProvider::default().build(),
        ])
        .commit()?;

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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // initialize the app state
            let app_handle = app.handle().clone();
            spawn({
                async move {
                    if let Err(e) = initialize(app_handle.clone()).await {
                        app_handle
                            .dialog()
                            .message(format!("Failed to initialize: {}", e))
                            .title("Error")
                            .kind(MessageDialogKind::Error)
                            .blocking_show();
                        std::process::exit(1);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![detection, ocr, inpaint])
        .run(tauri::generate_context!())?;

    Ok(())
}
