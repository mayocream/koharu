use std::sync::Arc;

use koharu_renderer::renderer::TextShaderEffect;
use tauri::State;
use tracing::warn;

use crate::{
    llm, ml,
    operations::{self, DocumentInput, InpaintRegion},
    renderer::Renderer,
    result::Result,
    state::{AppState, Document, TextBlock},
    version,
};

#[tauri::command]
pub fn open_external(url: &str) -> Result<()> {
    open::that(url)?;

    Ok(())
}

#[tauri::command]
pub async fn open_documents(state: State<'_, AppState>) -> Result<Vec<Document>> {
    let paths = rfd::FileDialog::new()
        .add_filter("Supported Files", &["khr", "png", "jpg", "jpeg", "webp"])
        .set_title("Pick Files")
        .pick_files()
        .unwrap_or_default();

    let inputs: Vec<DocumentInput> = paths
        .into_iter()
        .filter_map(|path| match std::fs::read(&path) {
            Ok(bytes) => Some(DocumentInput { path, bytes }),
            Err(err) => {
                warn!(?err, "Failed to read document");
                None
            }
        })
        .collect();

    let documents = operations::load_documents(inputs)?;
    operations::set_documents(&state, documents.clone()).await?;

    Ok(documents)
}

#[tauri::command]
pub fn app_version() -> String {
    version::current().to_string()
}

#[tauri::command]
pub async fn get_documents(state: State<'_, AppState>) -> Result<Vec<Document>> {
    operations::get_documents(&state).await
}

#[tauri::command]
pub async fn export_document(state: State<'_, AppState>, index: usize) -> Result<()> {
    let export = operations::export_document(&state, index).await?;
    let dest = rfd::FileDialog::new()
        .set_title("Select Export Destinition")
        .set_file_name(&export.filename)
        .save_file()
        .ok_or_else(|| anyhow::anyhow!("No file selected"))?;

    std::fs::write(dest, export.bytes)?;

    Ok(())
}

#[tauri::command]
pub async fn export_all_documents(state: State<'_, AppState>) -> Result<()> {
    let exports = operations::export_all_documents(&state).await?;
    let dest = rfd::FileDialog::new()
        .set_title("Select Export Destinition Folder")
        .pick_folder()
        .ok_or_else(|| anyhow::anyhow!("No directory selected"))?;

    for item in exports {
        std::fs::write(dest.join(&item.filename), item.bytes.as_slice())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn save_documents(state: State<'_, AppState>) -> Result<()> {
    let Some(default_filename) = operations::default_khr_filename(&state).await else {
        return Ok(());
    };

    let Some(dest) = rfd::FileDialog::new()
        .set_title("Save Koharu Document")
        .add_filter("Koharu Document", &["khr"])
        .set_file_name(default_filename)
        .save_file()
    else {
        return Ok(());
    };

    let bytes = operations::serialize_state(&state).await?;
    std::fs::write(dest, bytes)?;

    Ok(())
}

#[tauri::command]
pub async fn detect(
    state: State<'_, AppState>,
    model: State<'_, Arc<ml::Model>>,
    index: usize,
) -> Result<Document> {
    operations::detect(&state, &model, index).await
}

#[tauri::command]
pub async fn ocr(
    state: State<'_, AppState>,
    model: State<'_, Arc<ml::Model>>,
    index: usize,
) -> Result<Document> {
    operations::ocr(&state, &model, index).await
}

#[tauri::command]
pub async fn inpaint(
    state: State<'_, AppState>,
    model: State<'_, Arc<ml::Model>>,
    index: usize,
) -> Result<Document> {
    operations::inpaint(&state, &model, index).await
}

#[tauri::command]
pub async fn update_inpaint_mask(
    state: State<'_, AppState>,
    index: usize,
    mask: Vec<u8>,
    region: Option<InpaintRegion>,
) -> Result<Document> {
    operations::update_inpaint_mask(&state, index, mask, region).await
}

#[tauri::command]
pub async fn update_brush_layer(
    state: State<'_, AppState>,
    index: usize,
    patch: Vec<u8>,
    region: InpaintRegion,
) -> Result<Document> {
    operations::update_brush_layer(&state, index, patch, region).await
}

#[tauri::command]
pub async fn inpaint_partial(
    state: State<'_, AppState>,
    model: State<'_, Arc<ml::Model>>,
    index: usize,
    region: InpaintRegion,
) -> Result<Document> {
    operations::inpaint_partial(&state, &model, index, region).await
}

#[tauri::command]
pub async fn render(
    state: State<'_, AppState>,
    renderer: State<'_, Arc<Renderer>>,
    index: usize,
    text_block_index: Option<usize>,
    shader_effect: Option<TextShaderEffect>,
) -> Result<Document> {
    operations::render(&state, &renderer, index, text_block_index, shader_effect).await
}

#[tauri::command]
pub async fn update_text_blocks(
    state: State<'_, AppState>,
    index: usize,
    text_blocks: Vec<TextBlock>,
) -> Result<Document> {
    operations::update_text_blocks(&state, index, text_blocks).await
}

#[tauri::command]
pub fn list_font_families(renderer: State<'_, Arc<Renderer>>) -> Result<Vec<String>> {
    operations::list_font_families(&renderer)
}

#[tauri::command]
pub fn llm_list(model: State<'_, Arc<llm::Model>>) -> Vec<llm::ModelInfo> {
    operations::llm_list(&model)
}

#[tauri::command]
pub async fn llm_load(model: State<'_, Arc<llm::Model>>, id: String) -> Result<()> {
    operations::llm_load(&model, id).await
}

#[tauri::command]
pub async fn llm_offload(model: State<'_, Arc<llm::Model>>) -> Result<()> {
    operations::llm_offload(&model).await
}

#[tauri::command]
pub async fn llm_ready(model: State<'_, Arc<llm::Model>>) -> Result<bool> {
    operations::llm_ready(&model).await
}

#[tauri::command]
pub async fn llm_generate(
    state: State<'_, AppState>,
    model: State<'_, Arc<llm::Model>>,
    index: usize,
    text_block_index: Option<usize>,
    language: Option<String>,
) -> Result<Document> {
    operations::llm_generate(&state, &model, index, text_block_index, language).await
}
