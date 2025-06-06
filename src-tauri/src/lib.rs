use tauri_plugin_notification::NotificationExt;
use tokio::sync::RwLock;

use tauri::{AppHandle, Manager, async_runtime::spawn};

#[derive(Default)]
struct AppState {
    ctd: Option<comic_text_detector::ComicTextDetector>,
    ocr: Option<manga_ocr::MangaOCR>,
    lama: Option<lama::Lama>,
}

async fn initialize(app: AppHandle) -> anyhow::Result<()> {
    let state = app.state::<RwLock<AppState>>();
    let mut state = state.write().await;
    state.ctd = Some(comic_text_detector::ComicTextDetector::new()?);
    state.ocr = Some(manga_ocr::MangaOCR::new()?);
    state.lama = Some(lama::Lama::new()?);

    app.get_webview_window("splashscreen").unwrap().close()?;
    app.get_webview_window("main").unwrap().show()?;

    Ok(())
}

#[tauri::command]
async fn detect(
    state: tauri::State<'_, RwLock<AppState>>,
    image: Vec<u8>,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> Result<comic_text_detector::Output, String> {
    let state = state.read().await;
    let ctd = state
        .ctd
        .as_ref()
        .ok_or_else(|| "ComicTextDetector not initialized".to_string())?;

    let img = image::load_from_memory(&image).map_err(|e| e.to_string())?;
    let result = ctd
        .inference(&img, confidence_threshold, nms_threshold)
        .map_err(|e| e.to_string())?;

    Ok(result)
}

#[tauri::command]
async fn ocr(state: tauri::State<'_, RwLock<AppState>>, image: Vec<u8>) -> Result<String, String> {
    let state = state.read().await;
    let ocr = state
        .ocr
        .as_ref()
        .ok_or_else(|| "MangaOCR not initialized".to_string())?;

    let img = image::load_from_memory(&image).map_err(|e| e.to_string())?;
    let result = ocr.inference(&img).map_err(|e| e.to_string())?;

    Ok(result)
}

#[tauri::command]
async fn inpaint(
    state: tauri::State<'_, RwLock<AppState>>,
    image: Vec<u8>,
    mask: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let state = state.read().await;
    let lama = state
        .lama
        .as_ref()
        .ok_or_else(|| "LaMa not initialized".to_string())?;

    let img = image::load_from_memory(&image).map_err(|e| e.to_string())?;
    let mask = image::load_from_memory(&mask).map_err(|e| e.to_string())?;
    let result = lama.inference(&img, &mask).map_err(|e| e.to_string())?;

    // Convert the output image to array buffer
    let buf = result.as_bytes().to_vec();

    Ok(buf)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> anyhow::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .manage(RwLock::new(AppState::default()))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Debug)
                .level_for("ort::environment", log::LevelFilter::Info)
                .level_for(
                    "tao::platform_impl::platform::event_loop::runner",
                    log::LevelFilter::Error,
                )
                .build(),
        )
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
        .invoke_handler(tauri::generate_handler![detect, ocr, inpaint])
        .run(tauri::generate_context!())?;

    Ok(())
}
