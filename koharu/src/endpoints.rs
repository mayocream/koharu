use std::{io::Cursor, path::PathBuf, str::FromStr};

use anyhow::Result;
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use image::{GenericImageView, ImageFormat, RgbaImage};
use koharu_macros::endpoint;
use koharu_ml::llm::ModelId;
use koharu_renderer::renderer::TextShaderEffect;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use sys_locale::get_locale;
use tracing::instrument;

use crate::{
    image::SerializableDynamicImage,
    khr::{deserialize_khr, has_khr_magic, serialize_khr},
    llm,
    server::{ApiError, ApiResult, ApiState},
    state::{Document, TextBlock, TextStyle},
    version,
};

#[derive(Debug, Deserialize)]
pub struct FileInput {
    name: String,
    bytes: Vec<u8>,
}

#[endpoint(path = "/api/app_version", method = "get,post")]
pub async fn app_version(state: ApiState) -> Result<String> {
    Ok(version::current().to_string())
}

#[endpoint(path = "/api/open_external", method = "post")]
pub async fn open_external(url: String) -> Result<()> {
    open::that(&url)?;
    Ok(())
}

#[endpoint(path = "/api/get_documents", method = "get,post")]
pub async fn get_documents(state: ApiState) -> Result<usize> {
    let guard = state.app_state().read().await;
    Ok(guard.documents.len())
}

#[endpoint(path = "/api/get_document", method = "get,post")]
pub async fn get_document(state: ApiState, index: usize) -> Result<Document> {
    let guard = state.app_state().read().await;
    guard
        .documents
        .get(index)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {}", index))
}

#[endpoint(path = "/api/get_thumbnail", method = "get,post")]
pub async fn get_thumbnail(state: ApiState, index: usize) -> Result<Response> {
    let guard = state.app_state().read().await;
    let doc = guard
        .documents
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {}", index))?;

    let source = doc.rendered.as_ref().unwrap_or(&doc.image);
    let thumbnail = source.thumbnail(200, 200);

    let mut buf = Cursor::new(Vec::new());
    thumbnail.write_to(&mut buf, ImageFormat::WebP)?;

    let mut response = Response::new(Body::from(buf.into_inner()));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/webp"));
    Ok(response)
}

#[endpoint(path = "/api/open_documents", method = "post")]
pub async fn open_documents(state: ApiState, inputs: Vec<FileInput>) -> Result<usize> {
    if inputs.is_empty() {
        anyhow::bail!("No files uploaded");
    }

    let inputs: Vec<_> = inputs
        .into_iter()
        .map(|f| (PathBuf::from(f.name), f.bytes))
        .collect();

    let docs = load_documents(inputs)?;
    let count = docs.len();
    let mut guard = state.app_state().write().await;
    guard.documents = docs;
    Ok(count)
}

#[endpoint(path = "/api/save_documents", method = "post")]
pub async fn save_documents(state: ApiState) -> Result<Response> {
    let guard = state.app_state().read().await;
    if guard.documents.is_empty() {
        anyhow::bail!("No documents to save");
    }

    let filename = if guard.documents.len() == 1 {
        let stem = guard.documents[0]
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("project");
        format!("{}.khr", stem)
    } else {
        "project.khr".to_string()
    };

    let bytes = serialize_khr(&guard.documents)?;
    drop(guard);

    attachment_response(&filename, bytes, "application/octet-stream")
}

#[endpoint(path = "/api/export_document", method = "post")]
pub async fn export_document(state: ApiState, index: usize) -> Result<Response> {
    let (filename, bytes, ext) = {
        let guard = state.app_state().read().await;
        let document = guard
            .documents
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?;

        let ext = document
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg")
            .to_string();

        let rendered = document
            .rendered
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No rendered image found"))?;

        let bytes = encode_image(rendered, &ext)?;
        let filename = format!("{}_koharu.{}", document.name, ext);
        (filename, bytes, ext)
    };

    attachment_response(&filename, bytes, mime_from_ext(&ext))
}

#[endpoint(path = "/api/detect", method = "post")]
#[instrument(level = "info", skip_all)]
pub async fn detect(state: ApiState, index: usize) -> Result<()> {
    let snapshot = {
        let guard = state.app_state().read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let (text_blocks, segment) = state.ml().detect_dialog(&snapshot.image).await?;
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

        let font_predictions = state.ml().detect_fonts(&images, 1).await?;
        for (block, prediction) in updated.text_blocks.iter_mut().zip(font_predictions) {
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

    let mut guard = state.app_state().write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[endpoint(path = "/api/ocr", method = "post")]
#[instrument(level = "info", skip_all)]
pub async fn ocr(state: ApiState, index: usize) -> Result<()> {
    let snapshot = {
        let guard = state.app_state().read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let text_blocks = state
        .ml()
        .ocr(&snapshot.image, &snapshot.text_blocks)
        .await?;
    let mut updated = snapshot;
    updated.text_blocks = text_blocks;

    let mut guard = state.app_state().write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[endpoint(path = "/api/inpaint", method = "post")]
#[instrument(level = "info", skip_all)]
pub async fn inpaint(state: ApiState, index: usize) -> Result<()> {
    let snapshot = {
        let guard = state.app_state().read().await;
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
    let text_blocks = &snapshot.text_blocks;
    let mut segment_data = segment.to_rgba8();
    let (seg_width, seg_height) = segment_data.dimensions();

    for y in 0..seg_height {
        for x in 0..seg_width {
            let pixel = segment_data.get_pixel_mut(x, y);
            if pixel.0 != [0, 0, 0, 255] {
                let inside_any_block = text_blocks.iter().any(|block| {
                    x >= block.x as u32
                        && x < (block.x + block.width) as u32
                        && y >= block.y as u32
                        && y < (block.y + block.height) as u32
                });
                if !inside_any_block {
                    *pixel = image::Rgba([0, 0, 0, 255]);
                }
            }
        }
    }

    let mask = SerializableDynamicImage::from(image::DynamicImage::ImageRgba8(segment_data));
    let inpainted = state.ml().inpaint(&snapshot.image, &mask).await?;

    let mut updated = snapshot;
    updated.inpainted = Some(inpainted);

    let mut guard = state.app_state().write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[endpoint(path = "/api/update_inpaint_mask", method = "post")]
pub async fn update_inpaint_mask(
    state: ApiState,
    index: usize,
    mask: Vec<u8>,
    region: Option<InpaintRegion>,
) -> Result<()> {
    let snapshot = {
        let guard = state.app_state().read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let update_image = image::load_from_memory(&mask)?;
    let (doc_width, doc_height) = (snapshot.width, snapshot.height);

    let mut base_mask = snapshot
        .segment
        .clone()
        .unwrap_or_else(|| blank_rgba(doc_width, doc_height, image::Rgba([0, 0, 0, 255])))
        .to_rgba8();

    match region {
        Some(region) => {
            let (patch_width, patch_height) = update_image.dimensions();
            if patch_width != region.width || patch_height != region.height {
                anyhow::bail!(
                    "Mask patch size mismatch: expected {}x{}, got {}x{}",
                    region.width,
                    region.height,
                    patch_width,
                    patch_height
                );
            }

            let x0 = region.x.min(doc_width.saturating_sub(1));
            let y0 = region.y.min(doc_height.saturating_sub(1));
            let x1 = region.x.saturating_add(region.width).min(doc_width);
            let y1 = region.y.saturating_add(region.height).min(doc_height);

            if x1 <= x0 || y1 <= y0 {
                return Ok(());
            }

            let patch_rgba = update_image.to_rgba8();
            for y in 0..(y1 - y0) {
                for x in 0..(x1 - x0) {
                    base_mask.put_pixel(x0 + x, y0 + y, *patch_rgba.get_pixel(x, y));
                }
            }
        }
        None => {
            let (mask_width, mask_height) = update_image.dimensions();
            if mask_width != doc_width || mask_height != doc_height {
                anyhow::bail!(
                    "Mask size mismatch: expected {}x{}, got {}x{}",
                    doc_width,
                    doc_height,
                    mask_width,
                    mask_height
                );
            }
            base_mask = update_image.to_rgba8();
        }
    }

    let mut updated = snapshot;
    updated.segment = Some(image::DynamicImage::ImageRgba8(base_mask).into());

    let mut guard = state.app_state().write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[endpoint(path = "/api/update_brush_layer", method = "post")]
pub async fn update_brush_layer(
    state: ApiState,
    index: usize,
    patch: Vec<u8>,
    region: InpaintRegion,
) -> Result<()> {
    let snapshot = {
        let guard = state.app_state().read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let (img_width, img_height) = (snapshot.width, snapshot.height);
    let Some((x0, y0, width, height)) = clamp_region(&region, img_width, img_height) else {
        return Ok(());
    };

    let patch_image = image::load_from_memory(&patch)?;
    let (patch_width, patch_height) = patch_image.dimensions();

    if patch_width != region.width || patch_height != region.height {
        anyhow::bail!(
            "Brush patch size mismatch: expected {}x{}, got {}x{}",
            region.width,
            region.height,
            patch_width,
            patch_height
        );
    }

    let brush_rgba = patch_image.to_rgba8();
    let mut brush_layer = snapshot
        .brush_layer
        .clone()
        .unwrap_or_else(|| blank_rgba(img_width, img_height, image::Rgba([0, 0, 0, 0])))
        .to_rgba8();

    for y in 0..height {
        for x in 0..width {
            brush_layer.put_pixel(x0 + x, y0 + y, *brush_rgba.get_pixel(x, y));
        }
    }

    let mut updated = snapshot;
    updated.brush_layer = Some(image::DynamicImage::ImageRgba8(brush_layer).into());

    let mut guard = state.app_state().write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[endpoint(path = "/api/inpaint_partial", method = "post")]
#[instrument(level = "info", skip_all)]
pub async fn inpaint_partial(state: ApiState, index: usize, region: InpaintRegion) -> Result<()> {
    let snapshot = {
        let guard = state.app_state().read().await;
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
        return Ok(());
    }

    let (img_width, img_height) = (snapshot.width, snapshot.height);
    let x0 = region.x.min(img_width.saturating_sub(1));
    let y0 = region.y.min(img_height.saturating_sub(1));
    let x1 = region.x.saturating_add(region.width).min(img_width);
    let y1 = region.y.saturating_add(region.height).min(img_height);
    let crop_width = x1.saturating_sub(x0);
    let crop_height = y1.saturating_sub(y0);

    if crop_width == 0 || crop_height == 0 {
        return Ok(());
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
        return Ok(());
    }

    let image_crop =
        SerializableDynamicImage(snapshot.image.crop_imm(x0, y0, crop_width, crop_height));
    let mask_crop = SerializableDynamicImage(mask_image.crop_imm(x0, y0, crop_width, crop_height));

    let inpainted_crop = state.ml().inpaint(&image_crop, &mask_crop).await?;

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

    let mut guard = state.app_state().write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[endpoint(path = "/api/render", method = "post")]
#[instrument(level = "info", skip_all)]
pub async fn render(
    state: ApiState,
    index: usize,
    text_block_index: Option<usize>,
    shader_effect: Option<TextShaderEffect>,
) -> Result<()> {
    let snapshot = {
        let guard = state.app_state().read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let mut updated = snapshot;
    state.renderer().render(
        &mut updated,
        text_block_index,
        shader_effect.unwrap_or_default(),
    )?;

    let mut guard = state.app_state().write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[endpoint(path = "/api/update_text_blocks", method = "post")]
pub async fn update_text_blocks(
    state: ApiState,
    index: usize,
    text_blocks: Vec<TextBlock>,
) -> Result<()> {
    let mut guard = state.app_state().write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    document.text_blocks = text_blocks;
    Ok(())
}

#[endpoint(path = "/api/list_font_families", method = "get,post")]
pub async fn list_font_families(state: ApiState) -> Result<Vec<String>> {
    state.renderer().available_fonts()
}

#[endpoint(path = "/api/llm_list", method = "get,post")]
pub async fn llm_list(state: ApiState) -> Result<Vec<llm::ModelInfo>> {
    let mut models: Vec<ModelId> = ModelId::iter().collect();
    let cpu_factor = if state.llm().is_cpu() { 10 } else { 1 };
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

    Ok(models.into_iter().map(llm::ModelInfo::new).collect())
}

#[endpoint(path = "/api/llm_load", method = "post")]
#[instrument(level = "info", skip_all)]
pub async fn llm_load(state: ApiState, id: String) -> Result<()> {
    let id = ModelId::from_str(&id)?;
    state.llm().load(id).await;
    Ok(())
}

#[endpoint(path = "/api/llm_offload", method = "post")]
pub async fn llm_offload(state: ApiState) -> Result<()> {
    state.llm().offload().await;
    Ok(())
}

#[endpoint(path = "/api/llm_ready", method = "get,post")]
pub async fn llm_ready(state: ApiState) -> Result<bool> {
    Ok(state.llm().ready().await)
}

#[endpoint(path = "/api/llm_generate", method = "post")]
#[instrument(level = "info", skip_all)]
pub async fn llm_generate(
    state: ApiState,
    index: usize,
    text_block_index: Option<usize>,
    language: Option<String>,
) -> Result<()> {
    let snapshot = {
        let guard = state.app_state().read().await;
        guard
            .documents
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    if let Some(locale) = language.as_ref() {
        koharu_ml::set_locale(locale.clone());
    }

    let mut updated = snapshot;

    match text_block_index {
        Some(bi) => {
            let text_block = updated
                .text_blocks
                .get_mut(bi)
                .ok_or_else(|| anyhow::anyhow!("Text block not found"))?;
            state.llm().generate(text_block).await?;
        }
        None => {
            state.llm().generate(&mut updated).await?;
        }
    }

    let mut guard = state.app_state().write().await;
    let document = guard
        .documents
        .get_mut(index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

// Helpers

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

fn encode_image(image: &SerializableDynamicImage, ext: &str) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let format = ImageFormat::from_extension(ext).unwrap_or(ImageFormat::Jpeg);
    image.0.write_to(&mut cursor, format)?;
    Ok(buf)
}

fn attachment_response(filename: &str, bytes: Vec<u8>, content_type: &str) -> Result<Response> {
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_str(content_type)?);
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))?,
    );
    Ok(response)
}

pub fn load_documents(inputs: Vec<(PathBuf, Vec<u8>)>) -> Result<Vec<Document>> {
    if inputs.is_empty() {
        return Ok(vec![]);
    }

    if inputs.len() == 1 {
        let (_, ref bytes) = inputs[0];
        if has_khr_magic(bytes) {
            return deserialize_khr(bytes);
        }
    }

    let mut documents: Vec<_> = inputs
        .into_par_iter()
        .filter_map(|(path, bytes)| match Document::from_bytes(path, bytes) {
            Ok(docs) => Some(docs),
            Err(err) => {
                tracing::warn!(?err, "Failed to parse document");
                None
            }
        })
        .flatten()
        .collect();

    documents.sort_by_key(|doc| doc.name.clone());
    Ok(documents)
}

fn mime_from_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

fn blank_rgba(width: u32, height: u32, color: image::Rgba<u8>) -> SerializableDynamicImage {
    let blank = RgbaImage::from_pixel(width, height, color);
    image::DynamicImage::ImageRgba8(blank).into()
}
