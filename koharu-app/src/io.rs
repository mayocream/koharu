use std::path::{Path, PathBuf};

use image::ImageFormat;
use koharu_core::commands::{
    DeviceInfo, FileResult, OpenDocumentsPayload, OpenExternalPayload, ThumbnailResult,
};
use rfd::FileDialog;

use crate::AppResources;
use crate::utils::{encode_image, mime_from_ext};

fn next_available_path(output_dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut candidate = output_dir.join(format!("{stem}.{ext}"));
    let mut suffix = 2usize;
    while candidate.exists() {
        candidate = output_dir.join(format!("{stem}_{suffix}.{ext}"));
        suffix += 1;
    }
    candidate
}

async fn pick_output_dir() -> anyhow::Result<Option<PathBuf>> {
    Ok(tokio::task::spawn_blocking(|| FileDialog::new().pick_folder()).await?)
}

fn export_documents_matching(
    documents: &[koharu_core::Document],
    output_dir: &Path,
    suffix: &str,
    missing_error: &str,
    image: impl Fn(&koharu_core::Document) -> Option<&koharu_core::SerializableDynamicImage>,
) -> anyhow::Result<usize> {
    let mut exported = 0usize;

    for document in documents {
        let Some(image) = image(document) else {
            continue;
        };

        let ext = document
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("jpg")
            .to_string();
        let output_path =
            next_available_path(output_dir, &format!("{}_{}", document.name, suffix), &ext);
        let bytes = encode_image(image, &ext)?;
        std::fs::write(&output_path, bytes)?;
        exported += 1;
    }

    anyhow::ensure!(exported > 0, "{missing_error}");
    Ok(exported)
}

pub async fn app_version(state: AppResources) -> anyhow::Result<String> {
    Ok(state.version.to_string())
}

pub async fn device(state: AppResources) -> anyhow::Result<DeviceInfo> {
    Ok(DeviceInfo {
        ml_device: match state.device {
            koharu_ml::Device::Cpu => "CPU".to_string(),
            koharu_ml::Device::Cuda(_) => "CUDA".to_string(),
            koharu_ml::Device::Metal(_) => "Metal".to_string(),
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
    Ok(state.cache.list_documents()?.len())
}

pub async fn get_document(
    state: AppResources,
    document_id: &str,
) -> anyhow::Result<koharu_core::Document> {
    state.cache.get(document_id).await
}

pub async fn get_thumbnail(
    state: AppResources,
    document_id: &str,
) -> anyhow::Result<ThumbnailResult> {
    let doc = state.cache.get(document_id).await?;

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
    if payload.files.is_empty() {
        anyhow::bail!("No files uploaded");
    }

    let manifests = state.cache.import_files(payload.files)?;
    let count = manifests.len();
    state.cache.replace_manifests(&manifests).await?;
    Ok(count)
}

pub async fn add_documents(
    state: AppResources,
    payload: OpenDocumentsPayload,
) -> anyhow::Result<usize> {
    if payload.files.is_empty() {
        anyhow::bail!("No files uploaded");
    }

    let manifests = state.cache.import_files(payload.files)?;
    for manifest in &manifests {
        state.cache.save_manifest(manifest)?;
    }
    Ok(state.cache.list_documents()?.len())
}

pub async fn export_document(state: AppResources, document_id: &str) -> anyhow::Result<FileResult> {
    let document = state.cache.get(document_id).await?;

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

pub async fn export_all_inpainted(state: AppResources) -> anyhow::Result<usize> {
    let Some(output_dir) = pick_output_dir().await? else {
        return Ok(0);
    };

    let page_ids: Vec<String> = state
        .cache
        .list_documents()?
        .into_iter()
        .map(|e| e.id)
        .collect();
    let mut documents = Vec::new();
    for id in &page_ids {
        documents.push(state.cache.get(id).await?);
    }
    export_documents_matching(
        &documents,
        &output_dir,
        "inpainted",
        "No inpainted images found to export",
        |document| document.inpainted.as_ref(),
    )
}

pub async fn export_all_rendered(state: AppResources) -> anyhow::Result<usize> {
    let Some(output_dir) = pick_output_dir().await? else {
        return Ok(0);
    };

    let page_ids: Vec<String> = state
        .cache
        .list_documents()?
        .into_iter()
        .map(|e| e.id)
        .collect();
    let mut documents = Vec::new();
    for id in &page_ids {
        documents.push(state.cache.get(id).await?);
    }
    export_documents_matching(
        &documents,
        &output_dir,
        "rendered",
        "No rendered images found to export",
        |document| document.rendered.as_ref(),
    )
}
