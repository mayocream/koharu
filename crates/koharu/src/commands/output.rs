use std::{fs, sync::Arc};

use anyhow::{Context as _, Result};
use koharu_psd::{PsdExportOptions, export_page};
use koharu_renderer::{RasterOptions, Renderer};
use koharu_scene::{EntityId, PixelLayer, Snapshot};
use serde::Deserialize;
use specta::Type;
use tauri::{State, WebviewWindow, ipc::IpcResponse};

use super::{Error, project::CurrentProject};
use crate::desktop::Desktop;

const THUMBNAIL_EDGE: u32 = 128;

#[derive(Type)]
#[specta(transparent)]
pub(crate) struct ThumbnailBytes(#[specta(type = Vec<u8>)] Vec<u8>);

impl IpcResponse for ThumbnailBytes {
    fn body(self) -> tauri::Result<tauri::ipc::InvokeResponseBody> {
        Ok(self.0.into())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Png,
    Psd,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn export_pages(
    window: WebviewWindow,
    pages: Vec<EntityId>,
    format: ExportFormat,
    project: State<'_, CurrentProject>,
    desktop: State<'_, Desktop>,
) -> std::result::Result<(), Error> {
    let snapshot = {
        let project = project.project.lock();
        let project = project.as_ref().context("no project is open")?;
        project.flush()?;
        project.snapshot()
    };
    let Some(directory) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .pick_folder()
        .await
        .map(|directory| directory.path().to_owned())
    else {
        return Ok(());
    };
    let pages = if pages.is_empty() {
        snapshot.pages().map(|page| page.id()).collect()
    } else {
        pages
    };
    if pages.is_empty() {
        return Err(anyhow::anyhow!("there are no pages to export").into());
    }
    let renderer = desktop.renderer().clone();
    let options = RasterOptions::default();
    let concurrency = Arc::new(tokio::sync::Semaphore::new(2));
    let mut tasks = tokio::task::JoinSet::new();
    for (index, page_id) in pages.into_iter().enumerate() {
        let snapshot = snapshot.clone();
        let renderer = renderer.clone();
        let directory = directory.clone();
        let concurrency = Arc::clone(&concurrency);
        tasks.spawn(async move {
            let _permit = concurrency
                .acquire_owned()
                .await
                .context("the export queue was closed")?;
            let page = snapshot.page(page_id)?.page()?;
            let composition = renderer.compose(&snapshot, page_id).await?;
            let name = page
                .label
                .trim()
                .trim_end_matches(|character: char| character == '.' || character.is_whitespace());
            let name = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
            let name = name
                .chars()
                .map(|character| {
                    if matches!(
                        character,
                        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                    ) {
                        '_'
                    } else {
                        character
                    }
                })
                .collect::<String>();
            let stem = format!(
                "{:04}_{}",
                index + 1,
                if name.is_empty() { "page" } else { &name }
            );
            match format {
                ExportFormat::Png => {
                    let image = renderer.rasterize(&composition, &options).await?.image;
                    let path = directory.join(format!("{stem}.png"));
                    tokio::task::spawn_blocking(move || image.save(path))
                        .await
                        .context("the PNG encoder stopped unexpectedly")??;
                }
                ExportFormat::Psd => {
                    let bytes =
                        export_page(&renderer, &composition, &PsdExportOptions::default()).await?;
                    let path = directory.join(format!("{stem}.psd"));
                    tokio::task::spawn_blocking(move || fs::write(path, bytes))
                        .await
                        .context("the PSD writer stopped unexpectedly")??;
                }
            }
            Result::<()>::Ok(())
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.context("an export task stopped unexpectedly")??;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_thumbnail(
    page: EntityId,
    project: State<'_, CurrentProject>,
) -> std::result::Result<ThumbnailBytes, Error> {
    let snapshot = project
        .project
        .lock()
        .as_ref()
        .context("no project is open")?
        .snapshot();
    snapshot.page(page)?;
    let pixel = snapshot
        .component::<PixelLayer>(page)?
        .with_context(|| format!("page {page} has no pixel layer"))?;
    let blob = snapshot
        .asset(pixel.asset.owner, &pixel.asset.role)?
        .with_context(|| format!("page {page} has no visual asset"))?
        .blob;
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let bytes = snapshot.read_blob(blob)?;
        let image = image::load_from_memory(&bytes).context("failed to decode source image")?;
        if image.width() == 0 || image.height() == 0 {
            return Err(anyhow::anyhow!("source image is empty"));
        }
        let image = image.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE).to_rgba8();
        let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
        Ok(encoder.encode(80.0).to_vec())
    })
    .await
    .context("thumbnail worker stopped unexpectedly")??;
    Ok(ThumbnailBytes(bytes))
}

pub(crate) async fn rendered_preview(
    renderer: &Renderer,
    snapshot: &Snapshot,
    page: EntityId,
) -> Result<Vec<u8>> {
    snapshot.page(page)?;
    let composition = renderer.compose(snapshot, page).await?;
    let image = renderer
        .rasterize(&composition, &RasterOptions::default())
        .await?
        .image;
    tokio::task::spawn_blocking(move || {
        let image = image::DynamicImage::ImageRgba8(image)
            .resize(1024, 1024, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
        Ok(encoder.encode(85.0).to_vec())
    })
    .await
    .context("the preview encoder stopped unexpectedly")?
}
