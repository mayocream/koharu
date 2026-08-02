use std::fs;

use anyhow::{Context as _, Result};
use koharu_psd::{PsdExportOptions, export_page};
use koharu_renderer::{Compositor, RasterOptions, Rasterizer, RenderRequest, SceneRenderer};
use koharu_scene::{AssetRole, EntityId, Snapshot};
use rayon::prelude::*;
use serde::Deserialize;
use specta::Type;
use tauri::{State, WebviewWindow, ipc::IpcResponse};

use super::{Error, project::CurrentProject};

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
    let compositor = Compositor::new();
    let renderer = SceneRenderer::new();
    let rasterizer = Rasterizer::new().context("failed to initialize the export rasterizer")?;
    let options = RasterOptions::default();
    pages
        .into_par_iter()
        .enumerate()
        .try_for_each(|(index, page_id)| -> Result<()> {
            let page = snapshot.page(page_id)?.page()?;
            let composition = compositor.compile(&snapshot, &RenderRequest::new(page_id))?;
            let frame = renderer.render(&snapshot, &composition)?;
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
                ExportFormat::Png => rasterizer
                    .rasterize(&frame, options)?
                    .image
                    .save(directory.join(format!("{stem}.png")))?,
                ExportFormat::Psd => {
                    let bytes = export_page(
                        &snapshot,
                        &frame,
                        &rasterizer,
                        options,
                        &PsdExportOptions::default(),
                    )?;
                    fs::write(directory.join(format!("{stem}.psd")), bytes)?;
                }
            }
            Ok(())
        })?;
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
    let blob = snapshot
        .asset(page, &AssetRole::new("source")?)?
        .with_context(|| format!("page {page} has no source image"))?
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

pub(crate) fn rendered_preview(snapshot: &Snapshot, page: EntityId) -> Result<Vec<u8>> {
    snapshot.page(page)?;
    let composition = Compositor::new().compile(snapshot, &RenderRequest::new(page))?;
    let frame = SceneRenderer::new().render(snapshot, &composition)?;
    let rasterizer = Rasterizer::new().context("failed to initialize the preview rasterizer")?;
    let image = image::DynamicImage::ImageRgba8(
        rasterizer
            .rasterize(&frame, RasterOptions::default())?
            .image,
    )
    .resize(1024, 1024, image::imageops::FilterType::Lanczos3)
    .to_rgba8();
    let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
    Ok(encoder.encode(85.0).to_vec())
}
