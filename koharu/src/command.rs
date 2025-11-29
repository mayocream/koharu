use std::{str::FromStr, sync::Arc};

use koharu_ml::llm::ModelId;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use tauri::State;

use crate::{
    llm, ml,
    renderer::TextRenderer,
    result::Result,
    state::{AppState, Document, TextBlock},
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

    let mut documents = paths
        .into_par_iter()
        .filter_map(|path| Document::open(path).ok())
        .collect::<Vec<_>>();

    documents.sort_by_key(|doc| doc.name.clone());

    // store documents in app state
    let mut state = state.write().await;
    state.documents = documents.clone();

    // return opened documents as a copy
    Ok(documents)
}

#[tauri::command]
pub async fn detect(
    state: State<'_, AppState>,
    model: State<'_, Arc<ml::Model>>,
    index: usize,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let (text_blocks, segment) = model.detect(&document.image).await?;
    document.text_blocks = text_blocks;
    document.segment = Some(segment);

    Ok(document.clone())
}

#[tauri::command]
pub async fn ocr(
    state: State<'_, AppState>,
    model: State<'_, Arc<ml::Model>>,
    index: usize,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let text_blocks = model.ocr(&document.image, &document.text_blocks).await?;
    document.text_blocks = text_blocks;

    Ok(document.clone())
}

#[tauri::command]
pub async fn inpaint(
    state: State<'_, AppState>,
    model: State<'_, Arc<ml::Model>>,
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

    let inpainted = model.inpaint(&document.image, segment).await?;

    document.inpainted = Some(inpainted);

    Ok(document.clone())
}

#[tauri::command]
pub async fn render(
    state: State<'_, AppState>,
    renderer: State<'_, Arc<TextRenderer>>,
    index: usize,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    renderer.render(document).await?;

    Ok(document.clone())
}

#[tauri::command]
pub async fn update_text_blocks(
    state: State<'_, AppState>,
    index: usize,
    text_blocks: Vec<TextBlock>,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    document.text_blocks = text_blocks;

    Ok(document.clone())
}

#[tauri::command]
pub fn llm_list() -> Vec<String> {
    ModelId::all()
        .into_iter()
        .map(|id| id.to_string())
        .collect()
}

#[tauri::command]
pub async fn llm_load(model: State<'_, Arc<llm::Model>>, id: String) -> Result<()> {
    let id = ModelId::from_str(&id)?;
    model.load(id).await;
    Ok(())
}

#[tauri::command]
pub async fn llm_offload(model: State<'_, Arc<llm::Model>>) -> Result<()> {
    model.offload().await;
    Ok(())
}

#[tauri::command]
pub async fn llm_ready(model: State<'_, Arc<llm::Model>>) -> Result<bool> {
    Ok(model.ready().await)
}

#[tauri::command]
pub async fn llm_generate(
    state: State<'_, AppState>,
    model: State<'_, Arc<llm::Model>>,
    index: usize,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    model.generate(document).await?;

    Ok(document.clone())
}
