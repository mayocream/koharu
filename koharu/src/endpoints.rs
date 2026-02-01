use std::{io::Cursor, path::PathBuf, str::FromStr};

use axum::{
    Json,
    body::Body,
    extract::{Multipart, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use image::{GenericImageView, ImageFormat, RgbaImage};
use koharu_ml::llm::ModelId;
use koharu_renderer::renderer::{TextShaderEffect, WgpuDeviceInfo};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use sys_locale::get_locale;
use tracing::instrument;

use crate::{
    app::AppResources,
    image::SerializableDynamicImage,
    khr::{deserialize_khr, has_khr_magic, serialize_khr},
    llm,
    result::Result,
    state::{Document, TextBlock, TextStyle},
    version,
};

pub async fn app_version(State(_state): State<AppResources>) -> Result<Json<String>> {
    Ok(Json(version::current().to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub ml_device: String,
    pub wgpu: WgpuDeviceInfo,
}

pub async fn device(State(state): State<AppResources>) -> Result<Json<DeviceInfo>> {
    Ok(Json(DeviceInfo {
        ml_device: state.ml_device.to_string(),
        wgpu: state.renderer.wgpu_device_info(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenExternalPayload {
    pub url: String,
}

pub async fn open_external(Json(payload): Json<OpenExternalPayload>) -> Result<Json<()>> {
    open::that(&payload.url)?;
    Ok(Json(()))
}

pub async fn get_documents(State(state): State<AppResources>) -> Result<Json<usize>> {
    let guard = state.state.read().await;
    Ok(Json(guard.documents.len()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexPayload {
    pub index: usize,
}

pub async fn get_document(
    State(state): State<AppResources>,
    Json(payload): Json<IndexPayload>,
) -> Result<Json<Document>> {
    let guard = state.state.read().await;
    let doc = guard
        .documents
        .get(payload.index)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {}", payload.index))?;
    Ok(Json(doc))
}

pub async fn get_thumbnail(
    State(state): State<AppResources>,
    Json(payload): Json<IndexPayload>,
) -> Result<Response> {
    let guard = state.state.read().await;
    let doc = guard
        .documents
        .get(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {}", payload.index))?;

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

pub async fn open_documents(
    State(state): State<AppResources>,
    mut multipart: Multipart,
) -> Result<Json<usize>> {
    let mut inputs: Vec<(PathBuf, Vec<u8>)> = Vec::new();

    while let Some(field) = multipart.next_field().await? {
        let name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let bytes = field.bytes().await?.to_vec();
        inputs.push((PathBuf::from(name), bytes));
    }

    if inputs.is_empty() {
        Err(anyhow::anyhow!("No files uploaded"))?;
    }

    let docs = load_documents(inputs)?;
    let count = docs.len();
    let mut guard = state.state.write().await;
    guard.documents = docs;
    Ok(Json(count))
}

pub async fn save_documents(State(state): State<AppResources>) -> Result<Response> {
    let guard = state.state.read().await;
    if guard.documents.is_empty() {
        Err(anyhow::anyhow!("No documents to save"))?;
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

    Ok(attachment_response(
        &filename,
        bytes,
        "application/octet-stream",
    )?)
}

pub async fn export_document(
    State(state): State<AppResources>,
    Json(payload): Json<IndexPayload>,
) -> Result<Response> {
    let guard = state.state.read().await;
    let document = guard
        .documents
        .get(payload.index)
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
    drop(guard);

    Ok(attachment_response(&filename, bytes, mime_from_ext(&ext))?)
}

#[instrument(level = "info", skip_all)]
pub async fn detect(
    State(state): State<AppResources>,
    Json(payload): Json<IndexPayload>,
) -> Result<Json<()>> {
    let snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let (text_blocks, segment) = state.ml.detect_dialog(&snapshot.image).await?;
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

        let font_predictions = state.ml.detect_fonts(&images, 1).await?;
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

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(Json(()))
}

#[instrument(level = "info", skip_all)]
pub async fn ocr(
    State(state): State<AppResources>,
    Json(payload): Json<IndexPayload>,
) -> Result<Json<()>> {
    let snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let text_blocks = state.ml.ocr(&snapshot.image, &snapshot.text_blocks).await?;
    let mut updated = snapshot;
    updated.text_blocks = text_blocks;

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(Json(()))
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint(
    State(state): State<AppResources>,
    Json(payload): Json<IndexPayload>,
) -> Result<Json<()>> {
    let snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
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
    let inpainted = state.ml.inpaint(&snapshot.image, &mask).await?;

    let mut updated = snapshot;
    updated.inpainted = Some(inpainted);

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(Json(()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInpaintMaskPayload {
    pub index: usize,
    pub mask: Vec<u8>,
    pub region: Option<InpaintRegion>,
}

pub async fn update_inpaint_mask(
    State(state): State<AppResources>,
    Json(payload): Json<UpdateInpaintMaskPayload>,
) -> Result<Json<()>> {
    let snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let update_image = image::load_from_memory(&payload.mask)?;
    let (doc_width, doc_height) = (snapshot.width, snapshot.height);

    let mut base_mask = snapshot
        .segment
        .clone()
        .unwrap_or_else(|| blank_rgba(doc_width, doc_height, image::Rgba([0, 0, 0, 255])))
        .to_rgba8();

    match payload.region {
        Some(region) => {
            let (patch_width, patch_height) = update_image.dimensions();
            if patch_width != region.width || patch_height != region.height {
                Err(anyhow::anyhow!(
                    "Mask patch size mismatch: expected {}x{}, got {}x{}",
                    region.width,
                    region.height,
                    patch_width,
                    patch_height
                ))?;
            }

            let x0 = region.x.min(doc_width.saturating_sub(1));
            let y0 = region.y.min(doc_height.saturating_sub(1));
            let x1 = region.x.saturating_add(region.width).min(doc_width);
            let y1 = region.y.saturating_add(region.height).min(doc_height);

            if x1 <= x0 || y1 <= y0 {
                return Ok(Json(()));
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
                Err(anyhow::anyhow!(
                    "Mask size mismatch: expected {}x{}, got {}x{}",
                    doc_width,
                    doc_height,
                    mask_width,
                    mask_height
                ))?;
            }
            base_mask = update_image.to_rgba8();
        }
    }

    let mut updated = snapshot;
    updated.segment = Some(image::DynamicImage::ImageRgba8(base_mask).into());

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(Json(()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBrushLayerPayload {
    pub index: usize,
    pub patch: Vec<u8>,
    pub region: InpaintRegion,
}

pub async fn update_brush_layer(
    State(state): State<AppResources>,
    Json(payload): Json<UpdateBrushLayerPayload>,
) -> Result<Json<()>> {
    let snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let (img_width, img_height) = (snapshot.width, snapshot.height);
    let Some((x0, y0, width, height)) = clamp_region(&payload.region, img_width, img_height) else {
        return Ok(Json(()));
    };

    let patch_image = image::load_from_memory(&payload.patch)?;
    let (patch_width, patch_height) = patch_image.dimensions();

    if patch_width != payload.region.width || patch_height != payload.region.height {
        Err(anyhow::anyhow!(
            "Brush patch size mismatch: expected {}x{}, got {}x{}",
            payload.region.width,
            payload.region.height,
            patch_width,
            patch_height
        ))?;
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

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(Json(()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InpaintPartialPayload {
    pub index: usize,
    pub region: InpaintRegion,
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint_partial(
    State(state): State<AppResources>,
    Json(payload): Json<InpaintPartialPayload>,
) -> Result<Json<()>> {
    let snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let mask_image = snapshot
        .segment
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Segment image not found"))?;

    if payload.region.width == 0 || payload.region.height == 0 {
        return Ok(Json(()));
    }

    let (img_width, img_height) = (snapshot.width, snapshot.height);
    let x0 = payload.region.x.min(img_width.saturating_sub(1));
    let y0 = payload.region.y.min(img_height.saturating_sub(1));
    let x1 = payload
        .region
        .x
        .saturating_add(payload.region.width)
        .min(img_width);
    let y1 = payload
        .region
        .y
        .saturating_add(payload.region.height)
        .min(img_height);
    let crop_width = x1.saturating_sub(x0);
    let crop_height = y1.saturating_sub(y0);

    if crop_width == 0 || crop_height == 0 {
        return Ok(Json(()));
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
        return Ok(Json(()));
    }

    let image_crop =
        SerializableDynamicImage(snapshot.image.crop_imm(x0, y0, crop_width, crop_height));
    let mask_crop = SerializableDynamicImage(mask_image.crop_imm(x0, y0, crop_width, crop_height));

    let inpainted_crop = state.ml.inpaint(&image_crop, &mask_crop).await?;

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

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(Json(()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPayload {
    pub index: usize,
    pub text_block_index: Option<usize>,
    pub shader_effect: Option<TextShaderEffect>,
}

#[instrument(level = "info", skip_all)]
pub async fn render(
    State(state): State<AppResources>,
    Json(payload): Json<RenderPayload>,
) -> Result<Json<()>> {
    let snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let mut updated = snapshot;
    state.renderer.render(
        &mut updated,
        payload.text_block_index,
        payload.shader_effect.unwrap_or_default(),
    )?;

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(Json(()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTextBlocksPayload {
    pub index: usize,
    pub text_blocks: Vec<TextBlock>,
}

pub async fn update_text_blocks(
    State(state): State<AppResources>,
    Json(payload): Json<UpdateTextBlocksPayload>,
) -> Result<Json<()>> {
    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    document.text_blocks = payload.text_blocks;
    Ok(Json(()))
}

pub async fn list_font_families(State(state): State<AppResources>) -> Result<Json<Vec<String>>> {
    Ok(Json(state.renderer.available_fonts()?))
}

pub async fn llm_list(State(state): State<AppResources>) -> Result<Json<Vec<llm::ModelInfo>>> {
    let mut models: Vec<ModelId> = ModelId::iter().collect();
    let cpu_factor = if state.llm.is_cpu() { 10 } else { 1 };
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

    Ok(Json(models.into_iter().map(llm::ModelInfo::new).collect()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmLoadPayload {
    pub id: String,
}

#[instrument(level = "info", skip_all)]
pub async fn llm_load(
    State(state): State<AppResources>,
    Json(payload): Json<LlmLoadPayload>,
) -> Result<Json<()>> {
    let id = ModelId::from_str(&payload.id)?;
    state.llm.load(id).await;
    Ok(Json(()))
}

pub async fn llm_offload(State(state): State<AppResources>) -> Result<Json<()>> {
    state.llm.offload().await;
    Ok(Json(()))
}

pub async fn llm_ready(State(state): State<AppResources>) -> Result<Json<bool>> {
    Ok(Json(state.llm.ready().await))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmGeneratePayload {
    pub index: usize,
    pub text_block_index: Option<usize>,
    pub language: Option<String>,
}

#[instrument(level = "info", skip_all)]
pub async fn llm_generate(
    State(state): State<AppResources>,
    Json(payload): Json<LlmGeneratePayload>,
) -> Result<Json<()>> {
    let snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    if let Some(locale) = payload.language.as_ref() {
        koharu_ml::set_locale(locale.clone());
    }

    let mut updated = snapshot;

    match payload.text_block_index {
        Some(bi) => {
            let text_block = updated
                .text_blocks
                .get_mut(bi)
                .ok_or_else(|| anyhow::anyhow!("Text block not found"))?;
            state.llm.generate(text_block).await?;
        }
        None => {
            state.llm.generate(&mut updated).await?;
        }
    }

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(Json(()))
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

fn encode_image(image: &SerializableDynamicImage, ext: &str) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let format = ImageFormat::from_extension(ext).unwrap_or(ImageFormat::Jpeg);
    image.0.write_to(&mut cursor, format)?;
    Ok(buf)
}

fn attachment_response(
    filename: &str,
    bytes: Vec<u8>,
    content_type: &str,
) -> anyhow::Result<Response> {
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

pub fn load_documents(inputs: Vec<(PathBuf, Vec<u8>)>) -> anyhow::Result<Vec<Document>> {
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
