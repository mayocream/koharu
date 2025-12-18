use std::{str::FromStr, sync::Arc};

use koharu_ml::llm::ModelId;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use strum::IntoEnumIterator;
use sys_locale::get_locale;
use tauri::State;
use tracing::instrument;

use crate::{
    image::SerializableDynamicImage,
    llm, ml,
    renderer::Renderer,
    result::Result,
    state::{AppState, Document, TextBlock, TextStyle},
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

    let load_khr = paths.len() == 1
        && matches!(
            paths[0]
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
                .as_str(),
            "khr"
        );

    // khr loader or load image files
    let documents: Vec<Document> = if load_khr {
        let bytes = std::fs::read(&paths[0])?;
        deserialize_khr(&bytes).map_err(|e| anyhow::anyhow!("Failed to load documents: {e}"))?
    } else {
        let mut documents = paths
            .into_par_iter()
            .filter_map(|path| Document::open(path).ok())
            .collect::<Vec<_>>();

        documents.sort_by_key(|doc| doc.name.clone());

        documents
    };

    // store documents in app state
    let mut state = state.write().await;
    state.documents = documents.clone();

    // return opened documents as a copy
    Ok(documents)
}

#[tauri::command]
pub async fn export_document(state: State<'_, AppState>, index: usize) -> Result<()> {
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
        .map_err(|e| anyhow::anyhow!("Failed to save image: {e}"))?;

    Ok(())
}

#[tauri::command]
pub async fn export_all_documents(state: State<'_, AppState>) -> Result<()> {
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
            .map_err(|e| anyhow::anyhow!("Failed to save image: {e}"))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn save_documents(state: State<'_, AppState>) -> Result<()> {
    let state = state.read().await;

    if state.documents.is_empty() {
        return Ok(());
    }

    let default_filename = if state.documents.len() == 1 {
        // use the directory name of the document
        let stem = &state.documents[0]
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("project");
        format!("{}.khr", stem)
    } else {
        "project.khr".to_string()
    };

    let Some(dest) = rfd::FileDialog::new()
        .set_title("Save Koharu Document")
        .add_filter("Koharu Document", &["khr"])
        .set_file_name(default_filename)
        .save_file()
    else {
        return Ok(());
    };

    let bytes = serialize_khr(&state.documents)
        .map_err(|e| anyhow::anyhow!("Failed to serialize documents: {e}"))?;
    std::fs::write(dest, bytes)?;

    Ok(())
}

#[tauri::command]
#[instrument(level = "info", skip_all)]
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

    let (text_blocks, segment) = model.detect_dialog(&document.image).await?;
    document.text_blocks = text_blocks;
    document.segment = Some(segment);

    // detect fonts for each text block
    if !document.text_blocks.is_empty() {
        let images: Vec<image::DynamicImage> = document
            .text_blocks
            .iter()
            .map(|block| {
                document.image.crop_imm(
                    block.x as u32,
                    block.y as u32,
                    block.width as u32,
                    block.height as u32,
                )
            })
            .collect();
        let font_predictions = model.detect_fonts(&images, 1).await?;
        for (block, prediction) in document
            .text_blocks
            .iter_mut()
            .zip(font_predictions.into_iter())
        {
            tracing::debug!("Detected font for block {:?}: {:?}", block.text, prediction);

            // fill style with prediction, and use default font families for now
            let color = prediction.text_color.clone();
            let font_size = prediction.font_size_px.clone();

            block.font_prediction = Some(prediction);
            block.style = Some(TextStyle {
                font_size: font_size,
                color: [color[0], color[1], color[2], 255],
                ..Default::default()
            });
        }
    }

    Ok(document.clone())
}

#[tauri::command]
#[instrument(level = "info", skip_all)]
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
#[instrument(level = "info", skip_all)]
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
#[instrument(level = "info", skip_all)]
pub async fn render(
    state: State<'_, AppState>,
    renderer: State<'_, Arc<Renderer>>,
    index: usize,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    renderer.render(document)?;

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
pub fn llm_list(model: State<'_, Arc<llm::Model>>) -> Vec<String> {
    let mut models: Vec<ModelId> = ModelId::iter().collect();

    let cpu_factor = match model.is_cpu() {
        true => 10,
        false => 1,
    };

    let zh_locale_factor = match get_locale().unwrap_or_default() {
        locale if locale.starts_with("zh") => 10,
        _ => 1,
    };

    let non_zh_en_locale_factor = match get_locale().unwrap_or_default() {
        locale if locale.starts_with("zh") || locale.starts_with("en") => 1,
        _ => 100,
    };

    // sort models by language preference, the smaller the value, the higher the priority
    models.sort_by_key(|m| match m {
        ModelId::VntlLlama3_8Bv2 => 100,
        ModelId::Lfm2_350mEnjpMt => 200 / cpu_factor,
        ModelId::SakuraGalTransl7Bv3_7 => 300 / zh_locale_factor,
        ModelId::Sakura1_5bQwen2_5v1_0 => 400 / zh_locale_factor / cpu_factor,
        ModelId::HunyuanMT7B => 500 / non_zh_en_locale_factor,
    });

    models.into_iter().map(|id| id.to_string()).collect()
}

#[tauri::command]
#[instrument(level = "info", skip_all)]
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
#[instrument(level = "info", skip_all)]
pub async fn llm_generate(
    state: State<'_, AppState>,
    model: State<'_, Arc<llm::Model>>,
    index: usize,
    text_block_index: Option<usize>,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    match text_block_index {
        Some(bi) => {
            let text_block = document
                .text_blocks
                .get_mut(bi)
                .ok_or_else(|| anyhow::anyhow!("Text block not found"))?;

            model.generate(text_block).await?;
        }
        None => {
            model.generate(document).await?;
        }
    }

    Ok(document.clone())
}
