use std::path::{Path, PathBuf};

use koharu_core::FileEntry;
use koharu_ml::Device;
use rfd::FileDialog;

use crate::services::{AppResources, store};

use super::support::{encode_image, load_documents};

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

fn document_ext(document: &koharu_core::Document) -> String {
    document
        .path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("jpg")
        .to_string()
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

        let ext = document_ext(document);
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

pub fn device(state: &AppResources) -> &'static str {
    match state.device {
        Device::Cpu => "CPU",
        Device::Cuda(_) => "CUDA",
        Device::Metal(_) => "Metal",
    }
}

pub async fn get_documents(state: AppResources) -> anyhow::Result<usize> {
    let guard = state.state.read().await;
    Ok(guard.documents.len())
}

pub async fn get_document(
    state: AppResources,
    document_index: usize,
) -> anyhow::Result<koharu_core::Document> {
    store::read_doc(&state.state, document_index).await
}

pub async fn open_documents(state: AppResources, files: Vec<FileEntry>) -> anyhow::Result<usize> {
    let inputs: Vec<(PathBuf, Vec<u8>)> = files
        .into_iter()
        .map(|f| (PathBuf::from(f.name), f.data))
        .collect();

    if inputs.is_empty() {
        anyhow::bail!("No files uploaded");
    }

    let docs = load_documents(inputs)?;
    store::replace_docs(&state.state, docs).await
}

pub async fn add_documents(state: AppResources, files: Vec<FileEntry>) -> anyhow::Result<usize> {
    let inputs: Vec<(PathBuf, Vec<u8>)> = files
        .into_iter()
        .map(|f| (PathBuf::from(f.name), f.data))
        .collect();

    if inputs.is_empty() {
        anyhow::bail!("No files uploaded");
    }

    let docs = load_documents(inputs)?;
    store::append_docs(&state.state, docs).await
}

pub async fn export_document(
    state: AppResources,
    document_index: usize,
) -> anyhow::Result<Vec<u8>> {
    let document = store::read_doc(&state.state, document_index).await?;

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

    encode_image(rendered, &ext)
}

pub async fn export_all_inpainted(state: AppResources) -> anyhow::Result<usize> {
    let Some(output_dir) = pick_output_dir().await? else {
        return Ok(0);
    };

    let guard = state.state.read().await;
    export_documents_matching(
        &guard.documents,
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

    let guard = state.state.read().await;
    export_documents_matching(
        &guard.documents,
        &output_dir,
        "rendered",
        "No rendered images found to export",
        |document| document.rendered.as_ref(),
    )
}
