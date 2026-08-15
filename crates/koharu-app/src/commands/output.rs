use anyhow::{Context as _, Result};
use futures::{StreamExt as _, TryStreamExt as _, stream};
use image::{DynamicImage, ImageFormat};
use koharu_psd::{PsdExportOptions, export_page as export_psd_page};
use koharu_rasterizer::{Raster, RasterOptions, Rasterizer};
use koharu_renderer::{Frame, Renderer};
use koharu_scene::{AssetRole, EntityId, Snapshot};
use serde::Deserialize;
use specta::Type;
use std::{
    fs,
    io::{Cursor, Seek, Write},
    path::PathBuf,
    sync::Arc,
};
use tauri::{State, WebviewWindow, Wry, ipc::IpcResponse};
use zip::{ZipWriter, write::SimpleFileOptions};

use super::{Error, project::CurrentProject};
use koharu_desktop::Desktop;

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
pub enum PageExportFormat {
    Png,
    Psd,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProjectExportFormat {
    Png,
    Psd,
    Cbz,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportRequest {
    CurrentPage {
        page: EntityId,
        format: PageExportFormat,
    },
    EntireProject {
        format: ProjectExportFormat,
    },
}

struct ExportPage {
    id: EntityId,
    stem: String,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn export_pages(
    window: WebviewWindow<Wry>,
    request: ExportRequest,
    project: State<'_, CurrentProject>,
    desktop: State<'_, Desktop>,
) -> std::result::Result<(), Error> {
    let (snapshot, project_name) = {
        let project = project.project.lock().await;
        let project = project.as_ref().context("no project is open")?;
        (project.snapshot(), project.info().name)
    };
    match request {
        ExportRequest::CurrentPage { page, format } => {
            let page = export_page_job(&snapshot, page, None)?;
            let (label, extension) = page_format(format);
            let Some(path) = rfd::AsyncFileDialog::new()
                .set_parent(&window)
                .add_filter(label, &[extension])
                .set_file_name(format!("{}.{}", page.stem, extension))
                .save_file()
                .await
                .map(|file| file.path().to_owned())
            else {
                return Ok(());
            };
            let renderer = desktop.renderer();
            let rasterizer = desktop.rasterizer().await?;
            export_page_file(
                &snapshot,
                &renderer,
                Arc::clone(&rasterizer),
                page.id,
                format,
                path,
            )
            .await?;
        }
        ExportRequest::EntireProject { format } => {
            let pages = snapshot
                .pages()
                .enumerate()
                .map(|(index, page)| export_page_job(&snapshot, page.id(), Some(index)))
                .collect::<Result<Vec<_>>>()?;
            if pages.is_empty() {
                return Err(anyhow::anyhow!("there are no pages to export").into());
            }
            match format {
                ProjectExportFormat::Png | ProjectExportFormat::Psd => {
                    let Some(parent) = rfd::AsyncFileDialog::new()
                        .set_parent(&window)
                        .pick_folder()
                        .await
                        .map(|directory| directory.path().to_owned())
                    else {
                        return Ok(());
                    };
                    let directory = parent.join(&project_name);
                    tokio::fs::create_dir_all(&directory).await?;
                    let renderer = desktop.renderer();
                    let rasterizer = desktop.rasterizer().await?;
                    let format = match format {
                        ProjectExportFormat::Png => PageExportFormat::Png,
                        ProjectExportFormat::Psd => PageExportFormat::Psd,
                        ProjectExportFormat::Cbz => unreachable!(),
                    };
                    export_project_files(snapshot, renderer, rasterizer, pages, format, directory)
                        .await?;
                }
                ProjectExportFormat::Cbz => {
                    let Some(path) = rfd::AsyncFileDialog::new()
                        .set_parent(&window)
                        .add_filter("Comic Book Archive", &["cbz"])
                        .set_file_name(format!("{project_name}.cbz"))
                        .save_file()
                        .await
                        .map(|file| file.path().to_owned())
                    else {
                        return Ok(());
                    };
                    let renderer = desktop.renderer();
                    let rasterizer = desktop.rasterizer().await?;
                    export_project_cbz(snapshot, renderer, rasterizer, pages, path).await?;
                }
            }
        }
    }
    Ok(())
}

async fn export_project_files(
    snapshot: Snapshot,
    renderer: Renderer,
    rasterizer: Arc<Rasterizer>,
    pages: Vec<ExportPage>,
    format: PageExportFormat,
    directory: PathBuf,
) -> Result<()> {
    stream::iter(pages)
        .map(|page| {
            let renderer = renderer.clone();
            let rasterizer = Arc::clone(&rasterizer);
            let snapshot = snapshot.clone();
            let directory = directory.clone();
            async move {
                let (_, extension) = page_format(format);
                export_page_file(
                    &snapshot,
                    &renderer,
                    rasterizer,
                    page.id,
                    format,
                    directory.join(format!("{}.{}", page.stem, extension)),
                )
                .await
            }
        })
        .buffer_unordered(4)
        .try_collect::<Vec<_>>()
        .await?;
    Ok(())
}

async fn export_page_file(
    snapshot: &Snapshot,
    renderer: &Renderer,
    rasterizer: Arc<Rasterizer>,
    page: EntityId,
    format: PageExportFormat,
    path: PathBuf,
) -> Result<()> {
    let frame = renderer.render(snapshot, page).await?;
    match format {
        PageExportFormat::Png => {
            let image = rasterize(rasterizer, &frame, RasterOptions::default())
                .await?
                .image;
            tokio::task::spawn_blocking(move || image.save_with_format(path, ImageFormat::Png))
                .await
                .context("PNG export worker stopped unexpectedly")??;
        }
        PageExportFormat::Psd => {
            let bytes =
                export_psd_page(rasterizer, snapshot, &frame, &PsdExportOptions::default()).await?;
            tokio::fs::write(path, bytes).await?;
        }
    }
    Ok(())
}

async fn export_project_cbz(
    snapshot: Snapshot,
    renderer: Renderer,
    rasterizer: Arc<Rasterizer>,
    pages: Vec<ExportPage>,
    path: PathBuf,
) -> Result<()> {
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<(String, Vec<u8>)>(2);
    let archive_worker = tokio::task::spawn_blocking(move || -> Result<()> {
        let file = fs::File::create(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        let mut archive = ZipWriter::new(file);
        while let Some((name, bytes)) = receiver.blocking_recv() {
            append_cbz_entry(&mut archive, &name, &bytes)?;
        }
        archive.finish()?;
        Ok(())
    });

    let encoded_pages = stream::iter(pages)
        .map(|page| {
            let renderer = renderer.clone();
            let rasterizer = Arc::clone(&rasterizer);
            let snapshot = snapshot.clone();
            async move {
                let frame = renderer.render(&snapshot, page.id).await?;
                let image = rasterize(rasterizer, &frame, RasterOptions::default())
                    .await?
                    .image;
                let bytes = tokio::task::spawn_blocking(move || encode_png(image))
                    .await
                    .context("PNG encode worker stopped unexpectedly")??;
                Ok::<_, anyhow::Error>((format!("{}.png", page.stem), bytes))
            }
        })
        .buffered(4);
    futures::pin_mut!(encoded_pages);

    let export_result = async {
        while let Some(entry) = encoded_pages.try_next().await? {
            sender
                .send(entry)
                .await
                .map_err(|_| anyhow::anyhow!("CBZ archive writer stopped unexpectedly"))?;
        }
        Ok::<_, anyhow::Error>(())
    }
    .await
    .context("failed to prepare CBZ pages");
    drop(sender);

    let archive_result = archive_worker
        .await
        .context("CBZ archive writer stopped unexpectedly")?;
    archive_result?;
    export_result
}

fn export_page_job(snapshot: &Snapshot, id: EntityId, index: Option<usize>) -> Result<ExportPage> {
    let page = snapshot.page(id)?.page()?;
    let stem = page_stem(&page.label);
    Ok(ExportPage {
        id,
        stem: index.map_or(stem.clone(), |index| format!("{:04}_{stem}", index + 1)),
    })
}

fn page_stem(label: &str) -> String {
    let label = label
        .trim()
        .trim_end_matches(|character: char| character == '.' || character.is_whitespace());
    let label = label.rsplit_once('.').map_or(label, |(stem, _)| stem);
    let stem = label
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    if stem.is_empty() {
        "page".to_owned()
    } else {
        stem
    }
}

fn page_format(format: PageExportFormat) -> (&'static str, &'static str) {
    match format {
        PageExportFormat::Png => ("PNG Image", "png"),
        PageExportFormat::Psd => ("Photoshop Document", "psd"),
    }
}

fn encode_png(image: image::RgbaImage) -> Result<Vec<u8>> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

fn append_cbz_entry<W: Write + Seek>(
    archive: &mut ZipWriter<W>,
    name: &str,
    bytes: &[u8],
) -> Result<()> {
    archive.start_file(
        name,
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )?;
    archive.write_all(bytes)?;
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
        .await
        .as_ref()
        .context("no project is open")?
        .snapshot();
    snapshot.page(page)?;
    let blob = snapshot
        .asset(page, &AssetRole::new("source")?)?
        .with_context(|| format!("page {page} has no source image"))?
        .blob;
    let bytes = snapshot.read_blob(blob).await?;
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
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
    rasterizer: Arc<Rasterizer>,
    snapshot: &Snapshot,
    page: EntityId,
) -> Result<Vec<u8>> {
    snapshot.page(page)?;
    let frame = renderer.render(snapshot, page).await?;
    let image = rasterize(rasterizer, &frame, RasterOptions::default())
        .await?
        .image;
    tokio::task::spawn_blocking(move || {
        let image = image::DynamicImage::ImageRgba8(image)
            .resize(1024, 1024, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
        Ok::<_, anyhow::Error>(encoder.encode(85.0).to_vec())
    })
    .await
    .context("preview encode worker stopped unexpectedly")?
}

async fn rasterize(
    rasterizer: Arc<Rasterizer>,
    frame: &Frame,
    options: RasterOptions,
) -> Result<Raster> {
    let frame = frame.raster_frame()?;
    tokio::task::spawn_blocking(move || rasterizer.rasterize(&frame, options))
        .await
        .context("rasterizer worker stopped unexpectedly")?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use image::RgbaImage;

    use super::*;

    #[test]
    fn creates_safe_page_file_stems() {
        assert_eq!(page_stem("  Chapter 1.png  "), "Chapter 1");
        assert_eq!(page_stem("page:01?.webp"), "page_01_");
        assert_eq!(page_stem("..."), "page");
    }

    #[test]
    fn writes_png_images_into_cbz_entries() {
        let png = encode_png(RgbaImage::new(2, 3)).unwrap();
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        append_cbz_entry(&mut writer, "0001_Page 1.png", &png).unwrap();

        let cursor = writer.finish().unwrap();
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        assert_eq!(archive.len(), 1);
        let mut entry = archive.by_index(0).unwrap();
        assert_eq!(entry.name(), "0001_Page 1.png");
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        let image = image::load_from_memory_with_format(&bytes, ImageFormat::Png).unwrap();
        assert_eq!((image.width(), image.height()), (2, 3));
    }
}
