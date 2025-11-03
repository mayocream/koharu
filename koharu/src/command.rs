use std::sync::Arc;

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use tauri::State;

use crate::{
    inference::Inference,
    result::Result,
    state::{AppState, Document},
};

#[tauri::command]
pub fn open_external(url: &str) -> Result<()> {
    open::that(url)?;

    Ok(())
}

#[tauri::command]
pub async fn open_documents(state: State<'_, AppState>) -> Result<Vec<Document>> {
    let paths = rfd::FileDialog::new()
        .add_filter("Image Files", &["png", "jpg", "jpeg", "webp"])
        .add_filter("Koharu Document", &["khr"])
        .set_title("Pick Files")
        .pick_files()
        .unwrap_or_default();

    let documents = paths
        .into_par_iter()
        .filter_map(|path| Document::open(path).ok())
        .collect::<Vec<_>>();

    // store documents in app state
    let mut state = state.write().await;
    state.documents = documents.clone();

    // return opened documents as a copy
    Ok(documents)
}

#[tauri::command]
pub async fn detect(
    state: State<'_, AppState>,
    inference: State<'_, Arc<Inference>>,
    index: usize,
    conf_threshold: f32,
    nms_threshold: f32,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let (text_blocks, segment) = inference
        .detect(&document.image, conf_threshold, nms_threshold)
        .await?;
    document.text_blocks = text_blocks;
    document.segment = Some(segment);

    Ok(document.clone())
}

#[tauri::command]
pub async fn ocr(
    state: State<'_, AppState>,
    inference: State<'_, Arc<Inference>>,
    index: usize,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let text_blocks = inference
        .ocr(&document.image, &document.text_blocks)
        .await?;
    document.text_blocks = text_blocks;

    Ok(document.clone())
}

#[tauri::command]
pub async fn inpaint(
    state: State<'_, AppState>,
    inference: State<'_, Arc<Inference>>,
    index: usize,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let segment = document
        .segment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Segment image not found"))?;

    let inpainted = inference.inpaint(&document.image, segment).await?;
    document.inpainted = Some(inpainted);

    Ok(document.clone())
}
