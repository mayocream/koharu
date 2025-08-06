mod error;

use anyhow::Context;
use comic_text_detector::ComicTextDetector;
use lama::Lama;
use manga_ocr::MangaOCR;
use tauri::{AppHandle, Manager, async_runtime::spawn, command};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tokio::sync::RwLock;

use crate::error::Result;

#[derive(Debug)]
struct AppState {
    comic_text_detector: ComicTextDetector,
    manga_ocr: MangaOCR,
    lama: Lama,
}

#[command]
async fn detection(
    app: AppHandle,
    image: Vec<u8>,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> Result<comic_text_detector::Output> {
    let state = app.state::<RwLock<AppState>>();
    let mut state = state.write().await;

    let img = image::load_from_memory(&image).context("Failed to load image")?;

    let result = state
        .comic_text_detector
        .inference(&img, confidence_threshold, nms_threshold)
        .context("Failed to perform inference")?;

    Ok(result)
}

#[command]
async fn ocr(app: AppHandle, image: Vec<u8>) -> Result<String> {
    let state = app.state::<RwLock<AppState>>();
    let mut state = state.write().await;

    let img = image::load_from_memory(&image).context("Failed to load image")?;

    let result = state
        .manga_ocr
        .inference(&img)
        .context("Failed to perform OCR")?;

    Ok(result)
}

#[command]
async fn inpaint(app: AppHandle, image: Vec<u8>, mask: Vec<u8>) -> Result<Vec<u8>> {
    let state = app.state::<RwLock<AppState>>();
    let mut state = state.write().await;

    let img = image::load_from_memory(&image).context("Failed to load image")?;
    let mask_img = image::load_from_memory(&mask).context("Failed to load mask")?;

    let result = state
        .lama
        .inference(&img, &mask_img)
        .context("Failed to perform inpainting")?;

    Ok(result.into_bytes().to_vec())
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
