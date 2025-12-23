use std::{io::Cursor, path::PathBuf, str::FromStr, sync::Arc};

use image::{self, GenericImageView, ImageFormat, RgbaImage};
use koharu_ml::{llm::ModelId, set_locale};
use koharu_renderer::renderer::TextShaderEffect;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use sys_locale::get_locale;
use tracing::instrument;

use crate::{
    image::SerializableDynamicImage,
    khr::{deserialize_khr, has_khr_magic, serialize_khr},
    llm, ml,
    renderer::Renderer,
    result::Result,
    state::{AppState, Document, TextBlock, TextStyle},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InpaintRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct DocumentInput {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ExportedDocument {
    pub filename: String,
    pub bytes: Vec<u8>,
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

pub fn load_documents_from_paths(paths: Vec<PathBuf>) -> Result<Vec<Document>> {
    let inputs = paths
        .into_iter()
        .filter_map(|path| match std::fs::read(&path) {
            Ok(bytes) => Some(DocumentInput { path, bytes }),
            Err(err) => {
                tracing::warn!(?err, "Failed to read document at {:?}", path);
                None
            }
        })
        .collect();

    load_documents(inputs)
}

pub fn load_documents(inputs: Vec<DocumentInput>) -> Result<Vec<Document>> {
    if inputs.is_empty() {
        return Ok(vec![]);
    }

    if inputs.len() == 1 {
        let input = &inputs[0];
        if has_khr_magic(&input.bytes) {
            return Ok(deserialize_khr(&input.bytes)
                .map_err(|e| anyhow::anyhow!("Failed to load documents: {e}"))?);
        }
    }

    let mut documents = inputs
        .into_par_iter()
        .filter_map(
            |input| match Document::from_bytes(input.path, input.bytes) {
                Ok(docs) => Some(docs),
                Err(err) => {
                    tracing::warn!(?err, "Failed to parse document");
                    None
                }
            },
        )
        .flatten()
        .collect::<Vec<_>>();

    documents.sort_by_key(|doc| doc.name.clone());

    Ok(documents)
}

pub async fn set_documents(state: &AppState, documents: Vec<Document>) -> Result<Vec<Document>> {
    let mut guard = state.write().await;
    guard.documents = documents.clone();
    Ok(documents)
}

pub async fn get_documents(state: &AppState) -> Result<Vec<Document>> {
    let guard = state.read().await;
    Ok(guard.documents.clone())
}

pub async fn serialize_state(state: &AppState) -> Result<Vec<u8>> {
    let guard = state.read().await;
    let bytes = serialize_khr(&guard.documents)
        .map_err(|e| anyhow::anyhow!("Failed to serialize documents: {e}"))?;
    Ok(bytes)
}

pub fn serialize_documents(documents: &[Document]) -> Result<Vec<u8>> {
    let bytes = serialize_khr(documents)
        .map_err(|e| anyhow::anyhow!("Failed to serialize documents: {e}"))?;
    Ok(bytes)
}

pub async fn default_khr_filename(state: &AppState) -> Option<String> {
    let guard = state.read().await;
    if guard.documents.is_empty() {
        return None;
    }

    if guard.documents.len() == 1 {
        let stem = guard.documents[0]
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("project");
        Some(format!("{}.khr", stem))
    } else {
        Some("project.khr".to_string())
    }
}

pub async fn export_document(state: &AppState, index: usize) -> Result<ExportedDocument> {
    let guard = state.read().await;
    let document = guard
        .documents
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

    let document_ext = document
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("jpg");
    let default_filename = format!("{}_koharu.{}", document.name, document_ext);

    let rendered = document
        .rendered
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No inpainted image found"))?;

    let bytes = encode_image(rendered, document_ext)?;

    Ok(ExportedDocument {
        filename: default_filename,
        bytes,
    })
}

pub async fn export_all_documents(state: &AppState) -> Result<Vec<ExportedDocument>> {
    let guard = state.read().await;
    let mut exports = Vec::new();

    for document in &guard.documents {
        let document_ext = document
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("jpg");
        let default_filename = format!("{}_koharu.{}", document.name, document_ext);

        let rendered = document
            .rendered
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No inpainted image found"))?;

        let bytes = encode_image(rendered, document_ext)?;
        exports.push(ExportedDocument {
            filename: default_filename,
            bytes,
        });
    }

    Ok(exports)
}

#[instrument(level = "info", skip_all)]
pub async fn detect(state: &AppState, model: &Arc<ml::Model>, index: usize) -> Result<Document> {
    let snapshot = {
        let guard = state.read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let (text_blocks, segment) = model.detect_dialog(&snapshot.image).await?;
    let mut updated = snapshot.clone();
    updated.text_blocks = text_blocks;
    updated.segment = Some(segment);

    if !updated.text_blocks.is_empty() {
        let images: Vec<image::DynamicImage> = updated
            .text_blocks
            .iter()
            .map(|block| {
                updated.image.crop_imm(
                    block.x as u32,
                    block.y as u32,
                    block.width as u32,
                    block.height as u32,
                )
            })
            .collect();
        let font_predictions = model.detect_fonts(&images, 1).await?;
        for (block, prediction) in updated
            .text_blocks
            .iter_mut()
            .zip(font_predictions.into_iter())
        {
            tracing::debug!("Detected font for block {:?}: {:?}", block.text, prediction);

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

    let mut guard = state.write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated.clone();

    Ok(document.clone())
}

#[instrument(level = "info", skip_all)]
pub async fn ocr(state: &AppState, model: &Arc<ml::Model>, index: usize) -> Result<Document> {
    let snapshot = {
        let guard = state.read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let text_blocks = model.ocr(&snapshot.image, &snapshot.text_blocks).await?;

    let mut updated = snapshot;
    updated.text_blocks = text_blocks;

    let mut guard = state.write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated.clone();

    Ok(document.clone())
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint(state: &AppState, model: &Arc<ml::Model>, index: usize) -> Result<Document> {
    let snapshot = {
        let guard = state.read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let segment = snapshot
        .segment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Segment image not found"))?;

    let text_blocks = snapshot.text_blocks.clone();

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

    let inpainted = model.inpaint(&snapshot.image, &mask).await?;

    let mut updated = snapshot;
    updated.inpainted = Some(inpainted);

    let mut guard = state.write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated.clone();

    Ok(document.clone())
}

#[instrument(level = "info", skip_all)]
pub async fn update_inpaint_mask(
    state: &AppState,
    index: usize,
    mask: Vec<u8>,
    region: Option<InpaintRegion>,
) -> Result<Document> {
    let snapshot = {
        let guard = state.read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let update_image = image::load_from_memory(&mask)
        .map_err(|e| anyhow::anyhow!("Failed to decode mask: {e}"))?;

    let (doc_width, doc_height) = (snapshot.width, snapshot.height);

    let mut base_mask = snapshot
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
                return Ok(snapshot);
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

    let mut updated = snapshot;
    updated.segment = Some(image::DynamicImage::ImageRgba8(base_mask).into());

    let mut guard = state.write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated.clone();

    Ok(document.clone())
}

#[instrument(level = "info", skip_all)]
pub async fn update_brush_layer(
    state: &AppState,
    index: usize,
    patch: Vec<u8>,
    region: InpaintRegion,
) -> Result<Document> {
    let snapshot = {
        let guard = state.read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let (img_width, img_height) = (snapshot.width, snapshot.height);
    let Some((x0, y0, width, height)) = clamp_region(&region, img_width, img_height) else {
        return Ok(snapshot);
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

    let mut brush_layer = snapshot
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

    let mut updated = snapshot;
    updated.brush_layer = Some(image::DynamicImage::ImageRgba8(brush_layer).into());

    let mut guard = state.write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated.clone();

    Ok(document.clone())
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint_partial(
    state: &AppState,
    model: &Arc<ml::Model>,
    index: usize,
    region: InpaintRegion,
) -> Result<Document> {
    let snapshot = {
        let guard = state.read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let mask_image = snapshot
        .segment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Segment image not found"))?;

    if region.width == 0 || region.height == 0 {
        return Ok(snapshot);
    }

    let (img_width, img_height) = (snapshot.width, snapshot.height);
    let x0 = region.x.min(img_width.saturating_sub(1));
    let y0 = region.y.min(img_height.saturating_sub(1));
    let x1 = region.x.saturating_add(region.width).min(img_width);
    let y1 = region.y.saturating_add(region.height).min(img_height);
    let crop_width = x1.saturating_sub(x0);
    let crop_height = y1.saturating_sub(y0);

    if crop_width == 0 || crop_height == 0 {
        return Ok(snapshot);
    }

    let patch_x1 = x0 + crop_width;
    let patch_y1 = y0 + crop_height;

    let overlaps_text = snapshot.text_blocks.iter().any(|block| {
        let bx0 = block.x.max(0.0);
        let by0 = block.y.max(0.0);
        let bx1 = (block.x + block.width).max(bx0);
        let by1 = (block.y + block.height).max(by0);
        bx0 < patch_x1 as f32 && by0 < patch_y1 as f32 && bx1 > x0 as f32 && by1 > y0 as f32
    });

    if !overlaps_text {
        return Ok(snapshot);
    }

    let image_crop =
        SerializableDynamicImage(snapshot.image.crop_imm(x0, y0, crop_width, crop_height));
    let mask_crop = SerializableDynamicImage(mask_image.crop_imm(x0, y0, crop_width, crop_height));

    let inpainted_crop = model.inpaint(&image_crop, &mask_crop).await?;

    let mut stitched = snapshot
        .inpainted
        .as_ref()
        .unwrap_or(&snapshot.image)
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

    let mut updated = snapshot;
    updated.inpainted = Some(image::DynamicImage::ImageRgba8(stitched).into());

    let mut guard = state.write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated.clone();

    Ok(document.clone())
}

#[instrument(level = "info", skip_all)]
pub async fn render(
    state: &AppState,
    renderer: &Arc<Renderer>,
    index: usize,
    text_block_index: Option<usize>,
    shader_effect: Option<TextShaderEffect>,
) -> Result<Document> {
    let snapshot = {
        let guard = state.read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let mut updated = snapshot;
    renderer.render(
        &mut updated,
        text_block_index,
        shader_effect.unwrap_or_default(),
    )?;

    let mut guard = state.write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated.clone();

    Ok(document.clone())
}

pub async fn update_text_blocks(
    state: &AppState,
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

pub fn list_font_families(renderer: &Arc<Renderer>) -> Result<Vec<String>> {
    Ok(renderer.available_fonts()?)
}

pub fn llm_list(model: &Arc<llm::Model>) -> Vec<llm::ModelInfo> {
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

    models.sort_by_key(|m| match m {
        ModelId::VntlLlama3_8Bv2 => 100,
        ModelId::Lfm2_350mEnjpMt => 200 / cpu_factor,
        ModelId::SakuraGalTransl7Bv3_7 => 300 / zh_locale_factor,
        ModelId::Sakura1_5bQwen2_5v1_0 => 400 / zh_locale_factor / cpu_factor,
        ModelId::HunyuanMT7B => 500 / non_zh_en_locale_factor,
    });

    models.into_iter().map(llm::ModelInfo::new).collect()
}

#[instrument(level = "info", skip_all)]
pub async fn llm_load(model: &Arc<llm::Model>, id: String) -> Result<()> {
    let id = ModelId::from_str(&id)?;
    model.load(id).await;
    Ok(())
}

pub async fn llm_offload(model: &Arc<llm::Model>) -> Result<()> {
    model.offload().await;
    Ok(())
}

pub async fn llm_ready(model: &Arc<llm::Model>) -> Result<bool> {
    Ok(model.ready().await)
}

#[instrument(level = "info", skip_all)]
pub async fn llm_generate(
    state: &AppState,
    model: &Arc<llm::Model>,
    index: usize,
    text_block_index: Option<usize>,
    language: Option<String>,
) -> Result<Document> {
    let snapshot = {
        let guard = state.read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    if let Some(locale) = language.as_ref() {
        set_locale(locale.clone());
    }

    let mut updated = snapshot;

    match text_block_index {
        Some(bi) => {
            let text_block = updated
                .text_blocks
                .get_mut(bi)
                .ok_or_else(|| anyhow::anyhow!("Text block not found"))?;

            model.generate(text_block).await?;
        }
        None => {
            model.generate(&mut updated).await?;
        }
    }

    let mut guard = state.write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated.clone();

    Ok(document.clone())
}

fn encode_image(image: &SerializableDynamicImage, ext: &str) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let format = ImageFormat::from_extension(ext).unwrap_or(ImageFormat::Jpeg);
    image
        .0
        .write_to(&mut cursor, format)
        .map_err(|e| anyhow::anyhow!("Failed to encode image: {e}"))?;
    Ok(buf)
}
