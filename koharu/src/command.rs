use std::{str::FromStr, sync::Arc};

use koharu_models::llm::{GenerateOptions, ModelId};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use tauri::State;

use crate::{
    llm, onnx,
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
    model: State<'_, Arc<onnx::Model>>,
    index: usize,
    conf_threshold: f32,
    nms_threshold: f32,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let (text_blocks, segment) = model
        .detect(&document.image, conf_threshold, nms_threshold)
        .await?;
    document.text_blocks = text_blocks;
    document.segment = Some(segment);

    Ok(document.clone())
}

#[tauri::command]
pub async fn ocr(
    state: State<'_, AppState>,
    model: State<'_, Arc<onnx::Model>>,
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
    model: State<'_, Arc<onnx::Model>>,
    index: usize,
    dilate_kernel_size: u8,
    erode_distance: u8,
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

    let inpainted = model
        .inpaint(&document.image, segment, dilate_kernel_size, erode_distance)
        .await?;
    document.inpainted = Some(inpainted);

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
    prompt: &str,
) -> Result<Document> {
    let mut guard = model.get_mut().await;
    match &mut *guard {
        llm::State::Ready(llm) => {
            let mut state = state.write().await;
            let document = state
                .documents
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
            let messages: Vec<koharu_models::llm::ChatMessage> = llm::Prompt::new(
                prompt,
                document
                    .text_blocks
                    .clone()
                    .into_iter()
                    .map(|block| block.text.unwrap_or_else(|| "<empty>".to_string()))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .into();

            tracing::info!("Generating translation with messages: {:?}", messages);

            let response = llm.generate(&messages, &GenerateOptions::default())?;
            let translations = response.split("\n").collect::<Vec<_>>();
            for (block, translation) in document.text_blocks.iter_mut().zip(translations) {
                block.translation = Some(translation.to_string());
            }

            Ok(document.clone())
        }
        llm::State::Loading => Err(anyhow::anyhow!("Model is still loading").into()),
        llm::State::Failed(e) => Err(anyhow::anyhow!("Model failed to load: {}", e).into()),
        llm::State::Empty => Err(anyhow::anyhow!("No model is loaded").into()),
    }
}
