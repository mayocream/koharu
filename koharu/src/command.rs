use std::{str::FromStr, sync::Arc};

use image::{self, GenericImageView, RgbaImage};
use koharu_ml::{llm::ModelId, set_locale};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use sys_locale::get_locale;
use tauri::State;
use tracing::instrument;

use crate::{
    image::SerializableDynamicImage,
    khr::{deserialize_khr, has_khr_magic, serialize_khr},
    llm, ml,
    renderer::Renderer,
    result::Result,
    state::{AppState, Document, TextBlock, TextStyle},
    version,
};
use koharu_renderer::renderer::TextShaderEffect;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InpaintRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

fn clamp_region(region: &InpaintRegion, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let x0 = region.x.min(width.saturating_sub(1));
    let y0 = region.y.min(height.saturating_sub(1));
    let x1 = region.x.saturating_add(region.width).min(width).max(x0);
    let y1 = region.y.saturating_add(region.height).min(height).max(y0);
    let w = x1.saturating_sub(x0);
    let h = y1.saturating_sub(y0);
    if w == 0 || h == 0 {
        return None;
    }
    Some((x0, y0, w, h))
}

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

    let single_file_bytes = if paths.len() == 1 {
        Some(std::fs::read(&paths[0])?)
    } else {
        None
    };

    let load_images = |paths: Vec<std::path::PathBuf>| {
        let mut documents = paths
            .into_par_iter()
            .filter_map(|path| Document::open(path).ok())
            .collect::<Vec<_>>();

        documents.sort_by_key(|doc| doc.name.clone());

        documents
    };

    // khr loader or load image files
    let documents: Vec<Document> = if let Some(bytes) = single_file_bytes.as_ref() {
        if has_khr_magic(bytes) {
            deserialize_khr(bytes).map_err(|e| anyhow::anyhow!("Failed to load documents: {e}"))?
        } else {
            load_images(paths)
        }
    } else {
        load_images(paths)
    };

    // store documents in app state
    let mut state = state.write().await;
    state.documents = documents.clone();

    // return opened documents as a copy
    Ok(documents)
}

#[tauri::command]
pub fn app_version() -> String {
    version::current().to_string()
}

#[tauri::command]
pub async fn get_documents(state: State<'_, AppState>) -> Result<Vec<Document>> {
    let state = state.read().await;
    Ok(state.documents.clone())
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
            let color = prediction.text_color;
            let font_size = (prediction.font_size_px > 0.0).then_some(prediction.font_size_px);

            block.font_prediction = Some(prediction);
            block.style = Some(TextStyle {
                font_size,
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
pub async fn update_inpaint_mask(
    state: State<'_, AppState>,
    index: usize,
    mask: Vec<u8>,
    region: Option<InpaintRegion>,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let update_image = image::load_from_memory(&mask)
        .map_err(|e| anyhow::anyhow!("Failed to decode mask: {e}"))?;

    let (doc_width, doc_height) = (document.width, document.height);

    let mut base_mask = document
        .segment
        .clone()
        .unwrap_or_else(|| {
            let blank =
                image::RgbaImage::from_pixel(doc_width, doc_height, image::Rgba([0, 0, 0, 255]));
            image::DynamicImage::ImageRgba8(blank).into()
        })
        .to_rgba8();

    match region {
        Some(region) => {
            let (patch_width, patch_height) = update_image.dimensions();
            if patch_width != region.width || patch_height != region.height {
                return Err(anyhow::anyhow!(
                    "Mask patch size mismatch: expected {}x{}, got {}x{}",
                    region.width,
                    region.height,
                    patch_width,
                    patch_height
                )
                .into());
            }

            let x0 = region.x.min(doc_width.saturating_sub(1));
            let y0 = region.y.min(doc_height.saturating_sub(1));
            let x1 = region.x.saturating_add(region.width).min(doc_width);
            let y1 = region.y.saturating_add(region.height).min(doc_height);
            if x1 <= x0 || y1 <= y0 {
                return Ok(document.clone());
            }

            let dest_width = x1 - x0;
            let dest_height = y1 - y0;
            let patch_rgba = update_image.to_rgba8();
            for y in 0..dest_height {
                for x in 0..dest_width {
                    base_mask.put_pixel(x0 + x, y0 + y, *patch_rgba.get_pixel(x, y));
                }
            }
        }
        None => {
            let (mask_width, mask_height) = update_image.dimensions();
            if mask_width != doc_width || mask_height != doc_height {
                return Err(anyhow::anyhow!(
                    "Mask size mismatch: expected {}x{}, got {}x{}",
                    doc_width,
                    doc_height,
                    mask_width,
                    mask_height
                )
                .into());
            }

            base_mask = update_image.to_rgba8();
        }
    }

    document.segment = Some(image::DynamicImage::ImageRgba8(base_mask).into());

    Ok(document.clone())
}

#[tauri::command]
#[instrument(level = "info", skip_all)]
pub async fn update_brush_layer(
    state: State<'_, AppState>,
    index: usize,
    patch: Vec<u8>,
    region: InpaintRegion,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let (img_width, img_height) = (document.width, document.height);
    let Some((x0, y0, width, height)) = clamp_region(&region, img_width, img_height) else {
        return Ok(document.clone());
    };

    let patch_image = image::load_from_memory(&patch)
        .map_err(|e| anyhow::anyhow!("Failed to decode brush patch: {e}"))?;
    let (patch_width, patch_height) = patch_image.dimensions();
    if patch_width != region.width || patch_height != region.height {
        return Err(anyhow::anyhow!(
            "Brush patch size mismatch: expected {}x{}, got {}x{}",
            region.width,
            region.height,
            patch_width,
            patch_height
        )
        .into());
    }

    let brush_rgba = patch_image.to_rgba8();

    let mut brush_layer = document
        .brush_layer
        .clone()
        .unwrap_or_else(|| {
            let blank = RgbaImage::from_pixel(img_width, img_height, image::Rgba([0, 0, 0, 0]));
            image::DynamicImage::ImageRgba8(blank).into()
        })
        .to_rgba8();

    for y in 0..height {
        for x in 0..width {
            brush_layer.put_pixel(x0 + x, y0 + y, *brush_rgba.get_pixel(x, y));
        }
    }

    document.brush_layer = Some(image::DynamicImage::ImageRgba8(brush_layer).into());

    Ok(document.clone())
}

#[tauri::command]
#[instrument(level = "info", skip_all)]
pub async fn inpaint_partial(
    state: State<'_, AppState>,
    model: State<'_, Arc<ml::Model>>,
    index: usize,
    region: InpaintRegion,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let mask_image = document
        .segment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Segment image not found"))?;

    if region.width == 0 || region.height == 0 {
        return Ok(document.clone());
    }

    let (img_width, img_height) = (document.width, document.height);
    let x0 = region.x.min(img_width.saturating_sub(1));
    let y0 = region.y.min(img_height.saturating_sub(1));
    let x1 = region.x.saturating_add(region.width).min(img_width);
    let y1 = region.y.saturating_add(region.height).min(img_height);
    let crop_width = x1.saturating_sub(x0);
    let crop_height = y1.saturating_sub(y0);

    if crop_width == 0 || crop_height == 0 {
        return Ok(document.clone());
    }

    let patch_x1 = x0 + crop_width;
    let patch_y1 = y0 + crop_height;

    let overlaps_text = document.text_blocks.iter().any(|block| {
        let bx0 = block.x.max(0.0);
        let by0 = block.y.max(0.0);
        let bx1 = (block.x + block.width).max(bx0);
        let by1 = (block.y + block.height).max(by0);
        bx0 < patch_x1 as f32 && by0 < patch_y1 as f32 && bx1 > x0 as f32 && by1 > y0 as f32
    });

    if !overlaps_text {
        return Ok(document.clone());
    }

    let image_crop =
        SerializableDynamicImage(document.image.crop_imm(x0, y0, crop_width, crop_height));
    let mask_crop = SerializableDynamicImage(mask_image.crop_imm(x0, y0, crop_width, crop_height));

    let inpainted_crop = model.inpaint(&image_crop, &mask_crop).await?;

    // Restore erased regions from the original image; only copy inpainted pixels where mask is white.
    let mut stitched = document
        .inpainted
        .as_ref()
        .unwrap_or(&document.image)
        .to_rgba8();

    let patch = inpainted_crop.to_rgba8();
    let original = image_crop.to_rgba8();
    let mask_rgba = mask_crop.to_rgba8();
    for y in 0..crop_height {
        for x in 0..crop_width {
            let mask_pixel = mask_rgba.get_pixel(x, y);
            let is_masked = mask_pixel.0[0] > 0 || mask_pixel.0[1] > 0 || mask_pixel.0[2] > 0;
            let pixel = if is_masked {
                patch.get_pixel(x, y)
            } else {
                original.get_pixel(x, y)
            };
            stitched.put_pixel(x0 + x, y0 + y, *pixel);
        }
    }

    document.inpainted = Some(image::DynamicImage::ImageRgba8(stitched).into());

    Ok(document.clone())
}

#[tauri::command]
#[instrument(level = "info", skip_all)]
pub async fn render(
    state: State<'_, AppState>,
    renderer: State<'_, Arc<Renderer>>,
    index: usize,
    text_block_index: Option<usize>,
    shader_effect: Option<TextShaderEffect>,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    renderer.render(
        document,
        text_block_index,
        shader_effect.unwrap_or_default(),
    )?;

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
pub fn list_font_families(renderer: State<'_, Arc<Renderer>>) -> Result<Vec<String>> {
    Ok(renderer.available_fonts()?)
}

#[tauri::command]
pub fn llm_list(model: State<'_, Arc<llm::Model>>) -> Vec<llm::ModelInfo> {
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

    models.into_iter().map(llm::ModelInfo::new).collect()
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
    language: Option<String>,
) -> Result<Document> {
    let mut state = state.write().await;
    let document = state
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    if let Some(locale) = language.as_ref() {
        set_locale(locale.clone());
    }

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
