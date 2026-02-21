use std::path::PathBuf;

use image::ImageFormat;
use koharu_api::commands::{
    DeviceInfo, FileResult, IndexPayload, OpenDocumentsPayload, OpenExternalPayload,
    ThumbnailResult, WgpuDeviceInfo,
};

use crate::{AppResources, state_tx};

use super::utils::{encode_image, load_documents, mime_from_ext};

pub async fn app_version(state: AppResources) -> anyhow::Result<String> {
    Ok(state.version.to_string())
}

pub async fn device(state: AppResources) -> anyhow::Result<DeviceInfo> {
    let wgpu = state.renderer.wgpu_device_info();
    Ok(DeviceInfo {
        ml_device: match state.device {
            koharu_ml::Device::Cpu => "CPU".to_string(),
            koharu_ml::Device::Cuda(_) => "CUDA".to_string(),
            koharu_ml::Device::Metal(_) => "Metal".to_string(),
        },
        wgpu: WgpuDeviceInfo {
            name: wgpu.name,
            backend: wgpu.backend,
            device_type: wgpu.device_type,
            driver: wgpu.driver,
            driver_info: wgpu.driver_info,
        },
    })
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

pub async fn get_document(
    state: AppResources,
    payload: IndexPayload,
) -> anyhow::Result<koharu_types::Document> {
    state_tx::read_doc(&state.state, payload.index).await
}

pub async fn get_thumbnail(
    state: AppResources,
    payload: IndexPayload,
) -> anyhow::Result<ThumbnailResult> {
    let doc = state_tx::read_doc(&state.state, payload.index).await?;

    let source = doc.rendered.as_ref().unwrap_or(&doc.image);
    let thumbnail = source.thumbnail(200, 200);

    let mut buf = std::io::Cursor::new(Vec::new());
    thumbnail.write_to(&mut buf, ImageFormat::WebP)?;

    Ok(ThumbnailResult {
        data: buf.into_inner(),
        content_type: "image/webp".to_string(),
    })
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

pub async fn export_document(
    state: AppResources,
    payload: IndexPayload,
) -> anyhow::Result<FileResult> {
    let document = state_tx::read_doc(&state.state, payload.index).await?;

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

    Ok(FileResult {
        filename,
        data: bytes,
        content_type,
    })
}
