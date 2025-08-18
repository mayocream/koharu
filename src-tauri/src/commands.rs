use anyhow::Context;
use tauri::{AppHandle, Manager};
use tokio::sync::RwLock;

use crate::{AppState, error::CommandResult};

#[tauri::command]
pub async fn detection(
    app: AppHandle,
    image: Vec<u8>,
    confidence_threshold: f32,
    nms_threshold: f32,
) -> CommandResult<comic_text_detector::Output> {
    let state = app.state::<RwLock<AppState>>();
    let mut state = state.write().await;

    let img = image::load_from_memory(&image).context("Failed to load image")?;

    let result = state
        .comic_text_detector
        .inference(&img, confidence_threshold, nms_threshold)
        .context("Failed to perform inference")?;

    Ok(result)
}

#[tauri::command]
pub async fn ocr(app: AppHandle, image: Vec<u8>) -> CommandResult<String> {
    let state = app.state::<RwLock<AppState>>();
    let mut state = state.write().await;

    let img = image::load_from_memory(&image).context("Failed to load image")?;

    let result = state
        .manga_ocr
        .inference(&img)
        .context("Failed to perform OCR")?;

    Ok(result)
}

#[tauri::command]
pub async fn inpaint(app: AppHandle, image: Vec<u8>, mask: Vec<u8>) -> CommandResult<Vec<u8>> {
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
