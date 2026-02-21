use std::{io::Cursor, path::PathBuf, str::FromStr};

use image::{GenericImageView, ImageFormat, RgbaImage};
use koharu_ml::llm::ModelId;
use koharu_renderer::renderer::TextShaderEffect;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use tracing::instrument;

use koharu_types::{Document, SerializableDynamicImage, TextBlock};

use koharu_ml::llm::facade as llm;

use crate::AppResources;

pub async fn app_version(state: AppResources) -> anyhow::Result<String> {
    Ok(state.version.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub ml_device: String,
    pub wgpu: koharu_renderer::renderer::WgpuDeviceInfo,
}

pub async fn device(state: AppResources) -> anyhow::Result<DeviceInfo> {
    Ok(DeviceInfo {
        ml_device: match state.device {
            koharu_ml::Device::Cpu => "CPU".to_string(),
            koharu_ml::Device::Cuda(_) => "CUDA".to_string(),
            koharu_ml::Device::Metal(_) => "Metal".to_string(),
        },
        wgpu: state.renderer.wgpu_device_info(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenExternalPayload {
    pub url: String,
}

pub async fn open_external(
    _state: AppResources,
    payload: OpenExternalPayload,
) -> anyhow::Result<()> {
    open::that(&payload.url)?;
    Ok(())
}

pub async fn get_documents(state: AppResources) -> anyhow::Result<usize> {
    let guard = state.state.read().await;
    Ok(guard.documents.len())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexPayload {
    pub index: usize,
}

pub async fn get_document(state: AppResources, payload: IndexPayload) -> anyhow::Result<Document> {
    let guard = state.state.read().await;
    guard
        .documents
        .get(payload.index)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {}", payload.index))
}

/// Returns WebP-encoded thumbnail bytes.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailResult {
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
    pub content_type: String,
}

pub async fn get_thumbnail(
    state: AppResources,
    payload: IndexPayload,
) -> anyhow::Result<ThumbnailResult> {
    let guard = state.state.read().await;
    let doc = guard
        .documents
        .get(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found at index {}", payload.index))?;

    let source = doc.rendered.as_ref().unwrap_or(&doc.image);
    let thumbnail = source.thumbnail(200, 200);

    let mut buf = Cursor::new(Vec::new());
    thumbnail.write_to(&mut buf, ImageFormat::WebP)?;

    Ok(ThumbnailResult {
        data: buf.into_inner(),
        content_type: "image/webp".to_string(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocumentsPayload {
    pub files: Vec<FileEntry>,
}

pub async fn open_documents(
    state: AppResources,
    payload: OpenDocumentsPayload,
) -> anyhow::Result<usize> {
    let inputs: Vec<(PathBuf, Vec<u8>)> = payload
        .files
        .into_iter()
        .map(|f| (PathBuf::from(f.name), f.data))
        .collect();

    if inputs.is_empty() {
        anyhow::bail!("No files uploaded");
    }

    let docs = load_documents(inputs)?;
    let count = docs.len();
    let mut guard = state.state.write().await;
    guard.documents = docs;
    Ok(count)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileResult {
    pub filename: String,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
    pub content_type: String,
}

pub async fn export_document(
    state: AppResources,
    payload: IndexPayload,
) -> anyhow::Result<FileResult> {
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
    let content_type = mime_from_ext(&ext).to_string();
    drop(guard);

    Ok(FileResult {
        filename,
        data: bytes,
        content_type,
    })
}

#[instrument(level = "info", skip_all)]
pub async fn detect(state: AppResources, payload: IndexPayload) -> anyhow::Result<()> {
    let mut snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    state.ml.detect(&mut snapshot).await?;

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = snapshot;
    Ok(())
}

#[instrument(level = "info", skip_all)]
pub async fn ocr(state: AppResources, payload: IndexPayload) -> anyhow::Result<()> {
    let mut snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    state.ml.ocr(&mut snapshot).await?;

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = snapshot;
    Ok(())
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint(state: AppResources, payload: IndexPayload) -> anyhow::Result<()> {
    let mut snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    state.ml.inpaint(&mut snapshot).await?;

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = snapshot;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInpaintMaskPayload {
    pub index: usize,
    #[serde(with = "serde_bytes")]
    pub mask: Vec<u8>,
    pub region: Option<InpaintRegion>,
}

pub async fn update_inpaint_mask(
    state: AppResources,
    payload: UpdateInpaintMaskPayload,
) -> anyhow::Result<()> {
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

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBrushLayerPayload {
    pub index: usize,
    #[serde(with = "serde_bytes")]
    pub patch: Vec<u8>,
    pub region: InpaintRegion,
}

pub async fn update_brush_layer(
    state: AppResources,
    payload: UpdateBrushLayerPayload,
) -> anyhow::Result<()> {
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
        return Ok(());
    };

    let patch_image = image::load_from_memory(&payload.patch)?;
    let (patch_width, patch_height) = patch_image.dimensions();

    if patch_width != payload.region.width || patch_height != payload.region.height {
        anyhow::bail!(
            "Brush patch size mismatch: expected {}x{}, got {}x{}",
            payload.region.width,
            payload.region.height,
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

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InpaintPartialPayload {
    pub index: usize,
    pub region: InpaintRegion,
}

#[instrument(level = "info", skip_all)]
pub async fn inpaint_partial(
    state: AppResources,
    payload: InpaintPartialPayload,
) -> anyhow::Result<()> {
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
        return Ok(());
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

    let inpainted_crop = state.ml.inpaint_raw(&image_crop, &mask_crop).await?;

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
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPayload {
    pub index: usize,
    pub text_block_index: Option<usize>,
    pub shader_effect: Option<TextShaderEffect>,
    pub font_family: Option<String>,
}

#[instrument(level = "info", skip_all)]
pub async fn render(state: AppResources, payload: RenderPayload) -> anyhow::Result<()> {
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
        payload.font_family.as_deref(),
    )?;

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTextBlocksPayload {
    pub index: usize,
    pub text_blocks: Vec<TextBlock>,
}

pub async fn update_text_blocks(
    state: AppResources,
    payload: UpdateTextBlocksPayload,
) -> anyhow::Result<()> {
    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    document.text_blocks = payload.text_blocks;
    Ok(())
}

pub async fn list_font_families(state: AppResources) -> anyhow::Result<Vec<String>> {
    state.renderer.available_fonts()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmListPayload {
    pub language: Option<String>,
}

pub async fn llm_list(
    state: AppResources,
    payload: LlmListPayload,
) -> anyhow::Result<Vec<llm::ModelInfo>> {
    let mut models: Vec<ModelId> = ModelId::iter().collect();
    let cpu_factor = if state.llm.is_cpu() { 10 } else { 1 };
    let lang = payload.language.as_deref().unwrap_or("en");
    let zh_locale_factor = if lang.starts_with("zh") { 10 } else { 1 };
    let non_zh_en_locale_factor = if lang.starts_with("zh") || lang.starts_with("en") {
        1
    } else {
        100
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmLoadPayload {
    pub id: String,
}

#[instrument(level = "info", skip_all)]
pub async fn llm_load(state: AppResources, payload: LlmLoadPayload) -> anyhow::Result<()> {
    let id = ModelId::from_str(&payload.id)?;
    state.llm.load(id).await;
    Ok(())
}

pub async fn llm_offload(state: AppResources) -> anyhow::Result<()> {
    state.llm.offload().await;
    Ok(())
}

pub async fn llm_ready(state: AppResources) -> anyhow::Result<bool> {
    Ok(state.llm.ready().await)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmGeneratePayload {
    pub index: usize,
    pub text_block_index: Option<usize>,
    pub language: Option<String>,
}

#[instrument(level = "info", skip_all)]
pub async fn llm_generate(state: AppResources, payload: LlmGeneratePayload) -> anyhow::Result<()> {
    let snapshot = {
        let guard = state.state.read().await;
        guard
            .documents
            .get(payload.index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found"))?
    };

    let mut updated = snapshot;
    let target_language = payload.language.as_deref();

    match payload.text_block_index {
        Some(bi) => {
            let text_block = updated
                .text_blocks
                .get_mut(bi)
                .ok_or_else(|| anyhow::anyhow!("Text block not found"))?;
            state.llm.translate(text_block, target_language).await?;
        }
        None => {
            state.llm.translate(&mut updated, target_language).await?;
        }
    }

    let mut guard = state.state.write().await;
    let document = guard
        .documents
        .get_mut(payload.index)
        .ok_or_else(|| anyhow::anyhow!("Document not found"))?;
    *document = updated;
    Ok(())
}

// --- Auto-processing pipeline endpoints ---

pub async fn process(
    state: AppResources,
    payload: crate::pipeline::ProcessRequest,
) -> anyhow::Result<()> {
    {
        let guard = state.pipeline.read().await;
        if guard.is_some() {
            anyhow::bail!("A processing pipeline is already running");
        }
    }

    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut guard = state.pipeline.write().await;
        *guard = Some(crate::pipeline::PipelineHandle {
            cancel: cancel.clone(),
        });
    }

    let resources = state.clone();
    tokio::spawn(async move {
        crate::pipeline::run_pipeline(resources, payload, cancel).await;
    });

    Ok(())
}

pub async fn process_cancel(state: AppResources) -> anyhow::Result<()> {
    let guard = state.pipeline.read().await;
    if let Some(handle) = guard.as_ref() {
        handle
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}

// --- Helpers ---

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

fn mime_from_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

pub fn load_documents(inputs: Vec<(PathBuf, Vec<u8>)>) -> anyhow::Result<Vec<Document>> {
    if inputs.is_empty() {
        return Ok(vec![]);
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

fn blank_rgba(width: u32, height: u32, color: image::Rgba<u8>) -> SerializableDynamicImage {
    let blank = RgbaImage::from_pixel(width, height, color);
    image::DynamicImage::ImageRgba8(blank).into()
}
