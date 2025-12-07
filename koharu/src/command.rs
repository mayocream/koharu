use std::{str::FromStr, sync::Arc};

use koharu_ml::llm::ModelId;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use strum::IntoEnumIterator;
use sys_locale::get_locale;
use tauri::State;

use crate::{
    image::SerializableDynamicImage,
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
pub async fn save_document(state: State<'_, AppState>, index: usize) -> Result<()> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let document_ext = document
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("jpg");
    let default_filename = format!("{}_koharu.{}", document.name, document_ext);

    let dest = rfd::FileDialog::new()
        .set_title("Select Export Destinition")
        // default as filename_koharu.file_ext
        .set_file_name(default_filename)
        .save_file()
        .ok_or_else(|| anyhow::anyhow!("No file selected"))?;

    document
        .rendered
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No inpainted image found"))?
        .save(&dest)
        .or_else(|e| Err(anyhow::anyhow!("Failed to save image: {}", e)))?;

    Ok(())
}

#[tauri::command]
pub async fn save_all_documents(state: State<'_, AppState>) -> Result<()> {
    let dest = rfd::FileDialog::new()
        .set_title("Select Export Destinition Folder")
        // default as filename_koharu.file_ext
        .pick_folder()
        .ok_or_else(|| anyhow::anyhow!("No directory selected"))?;

    let state = state.read().await;

    let documents = state.documents.iter();

    for document in documents {
        let document_ext = document
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("jpg");
        let default_filename = format!("{}_koharu.{}", document.name, document_ext);

        document
            .rendered
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No inpainted image found"))?
            // save to dest/default_filename
            .save(dest.join(&default_filename))
            .or_else(|e| Err(anyhow::anyhow!("Failed to save image: {}", e)))?;
    }

    Ok(())
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

    let text_blocks = document.text_blocks.clone();

    // for every pixel in segment_ref that is not black, check if it's inside any text block, else set to black
    let mut segment_data = segment.to_rgba8();
    let (seg_width, seg_height) = segment_data.dimensions();
    for y in 0..seg_height {
        for x in 0..seg_width {
            let pixel = segment_data.get_pixel_mut(x, y);
            if pixel.0 != [0, 0, 0, 255] {
                let mut inside_any_block = false;
                for block in &text_blocks {
                    if x >= block.x as u32
                        && x < (block.x + block.width) as u32
                        && y >= block.y as u32
                        && y < (block.y + block.height) as u32
                    {
                        inside_any_block = true;
                        break;
                    }
                }
                if !inside_any_block {
                    *pixel = image::Rgba([0, 0, 0, 255]);
                }
            }
        }
    }

    let mask = SerializableDynamicImage::from(image::DynamicImage::ImageRgba8(segment_data));

    let inpainted = model.inpaint(&document.image, &mask).await?;

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
    let mut models: Vec<ModelId> = ModelId::iter().collect();

    match get_locale() {
        Some(locale) => {
            println!("Current locale: {}", locale);

            if locale.starts_with("zh") {
                models.sort_by_key(|m| match m {
                    ModelId::VntlLlama3_8Bv2 => 2,
                    ModelId::Lfm2_350mEnjpMt => 3,
                    ModelId::SakuraGalTransl7Bv3_7 => 0,
                    ModelId::Sakura1_5bQwen2_5v1_0 => 1,
                });
            }
            // add more condition if more languages are supported
        }
        None => {
            // default ordering for english
        }
    }

    models.into_iter().map(|id| id.to_string()).collect()
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
