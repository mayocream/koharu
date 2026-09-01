use anyhow::{Context as _, Result};
use futures::{StreamExt as _, TryStreamExt as _, stream};
use image::{
    ExtendedColorType, ImageEncoder as _,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use koharu_psd::{PsdExportOptions, export_page};
use koharu_rasterizer::{Raster, RasterOptions, Rasterizer};
use koharu_renderer::{Frame, Renderer};
use koharu_scene::{AssetRole, EntityId, Snapshot};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::{Cef, State, WebviewWindow, ipc::IpcResponse};

use super::{ChannelExt as _, Error, canvas::CanvasChannel, project::CurrentProject};
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
pub enum ExportFormat {
    Png,
    Psd,
}

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextExportKind {
    Source,
    Translation,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct TextExport {
    pages: Vec<TextExportPage>,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct TextExportPage {
    page: usize,
    texts: Vec<String>,
}

#[derive(Debug, Serialize, Type)]
pub(crate) struct ImportTextsResult {
    pub applied: u32,
    pub skipped: Vec<ImportTextsSkip>,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, Type)]
pub(crate) struct ImportTextsSkip {
    pub page: u32,
    pub reason: String,
}

fn serialize_text_export(pages: Vec<TextExportPage>) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(&TextExport { pages })?)
}

fn text_export_page(page: usize, texts: Vec<String>) -> Option<TextExportPage> {
    texts
        .iter()
        .any(|text| !text.trim().is_empty())
        .then_some(TextExportPage { page, texts })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn export_texts(
    window: WebviewWindow<Cef>,
    pages: Vec<EntityId>,
    project: State<'_, CurrentProject>,
    export_kind: TextExportKind,
) -> Result<(), Error> {
    let snapshot = {
        let project = project.project.lock().await;
        let project = project.as_ref().context("no project is open")?;
        project.snapshot()
    };
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .set_file_name(match export_kind {
            TextExportKind::Source => "source-texts.json",
            TextExportKind::Translation => "translations.json",
        })
        .save_file()
        .await
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

    let mut exported_pages = Vec::with_capacity(pages.len());
    for (page_index, page_id) in pages.into_iter().enumerate() {
        let page = snapshot.page(page_id)?;
        let mut texts = Vec::new();

        if let Some(text_group) = page.text_group()? {
            for layer in text_group.text_layers()? {
                let content = layer.content()?;
                let Some(source) = content.source()? else {
                    let text = match export_kind {
                        TextExportKind::Source => String::new(),
                        TextExportKind::Translation => content
                            .translation()?
                            .map_or_else(String::new, |text| text.text.value),
                    };
                    texts.push(text);
                    continue;
                };
                let text = match export_kind {
                    TextExportKind::Source => source.text.value,
                    TextExportKind::Translation => content
                        .translation()?
                        .map_or_else(String::new, |text| text.text.value),
                };
                texts.push(text);
            }
        }

        if let Some(page) = text_export_page(page_index + 1, texts) {
            exported_pages.push(page);
        }
    }

    let bytes = serialize_text_export(exported_pages)?;
    tokio::fs::write(file.path(), bytes).await?;
    Ok(())
}

fn extract_json(input: &str) -> Option<&str> {
    if let Some(start) = input.find("```json") {
        let after = &input[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim());
        }
    }
    if let Some(start) = input.find("```") {
        let after = &input[start + 3..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim());
        }
    }
    if let (Some(open), Some(close)) = (input.find('{'), input.rfind('}'))
        && close > open
    {
        Some(input[open..=close].trim())
    } else {
        None
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn import_texts(
    window: WebviewWindow<Cef>,
    project: State<'_, CurrentProject>,
    desktop: State<'_, Desktop>,
    canvas_channel: State<'_, CanvasChannel>,
    import_kind: TextExportKind,
) -> Result<ImportTextsResult, Error> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter("JSON", &["json"])
        .pick_file()
        .await
    else {
        return Ok(ImportTextsResult {
            applied: 0,
            skipped: Vec::new(),
            errors: Vec::new(),
        });
    };

    let bytes = tokio::fs::read(file.path()).await?;
    let input = String::from_utf8(bytes).context("the selected file is not valid UTF-8")?;
    let Some(json) = extract_json(&input) else {
        return Ok(ImportTextsResult {
            applied: 0,
            skipped: Vec::new(),
            errors: vec!["could not locate a JSON object in the selected file".to_owned()],
        });
    };
    let export: TextExport = match serde_json::from_str(json) {
        Ok(export) => export,
        Err(error) => {
            return Ok(ImportTextsResult {
                applied: 0,
                skipped: Vec::new(),
                errors: vec![format!("failed to parse JSON: {error}")],
            });
        }
    };

    let (commit, page, result) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let snapshot = project.snapshot();
        let page_ids = snapshot.pages().map(|page| page.id()).collect::<Vec<_>>();
        let mut last_commit = None;
        let mut revisions = Vec::new();
        let mut applied = 0_u32;
        let mut skipped = Vec::new();

        for (page_index, page_id) in page_ids.into_iter().enumerate() {
            let page_number = page_index + 1;
            let page = snapshot.page(page_id)?;
            let mut text_layers = Vec::new();
            let group = match page.text_group() {
                Ok(group) => group,
                Err(error) => {
                    skipped.push(ImportTextsSkip {
                        page: page_number as u32,
                        reason: format!("failed to read text group: {error}"),
                    });
                    continue;
                }
            };
            if let Some(group) = group {
                let layers = match group.text_layers() {
                    Ok(layers) => layers,
                    Err(error) => {
                        skipped.push(ImportTextsSkip {
                            page: page_number as u32,
                            reason: format!("failed to read text layers: {error}"),
                        });
                        continue;
                    }
                };
                for layer in layers {
                    text_layers.push(layer.id());
                }
            }

            if text_layers.is_empty() {
                continue;
            }
            let Some(page_export) = export.pages.iter().find(|page| page.page == page_number)
            else {
                continue;
            };
            if page_export.texts.len() != text_layers.len() {
                skipped.push(ImportTextsSkip {
                    page: page_number as u32,
                    reason: format!(
                        "text count mismatch: expected {}, got {}",
                        text_layers.len(),
                        page_export.texts.len()
                    ),
                });
                continue;
            }

            for (layer, text) in text_layers.into_iter().zip(&page_export.texts) {
                let commit = match import_kind {
                    TextExportKind::Source => project.set_source_text(layer, text.clone()).await,
                    TextExportKind::Translation => {
                        project.set_translation(layer, Some(text.clone())).await
                    }
                };
                match commit {
                    Ok(commit) => {
                        revisions.push(commit.revision);
                        last_commit = Some(commit);
                    }
                    Err(error) => {
                        skipped.push(ImportTextsSkip {
                            page: page_number as u32,
                            reason: format!("failed to apply text: {error}"),
                        });
                        break;
                    }
                };
            }
            applied += 1;
        }
        project.record(revisions);

        (
            last_commit,
            project.active_page(),
            ImportTextsResult {
                applied,
                skipped,
                errors: Vec::new(),
            },
        )
    };

    if let Some(commit) = commit {
        desktop.synchronize(&commit.snapshot, page, &commit).await?;
        canvas_channel.channel.publish(desktop.canvas_state());
    }
    Ok(result)
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "export",
    skip_all,
    fields(origin = "user", format = ?format),
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn export_pages(
    window: WebviewWindow<Cef>,
    pages: Vec<EntityId>,
    format: ExportFormat,
    project: State<'_, CurrentProject>,
    desktop: State<'_, Desktop>,
) -> std::result::Result<(), Error> {
    let snapshot = {
        let project = project.project.lock().await;
        let project = project.as_ref().context("no project is open")?;
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
    let renderer = desktop.renderer();
    let rasterizer = desktop.rasterizer().await?;
    let jobs = pages
        .into_iter()
        .enumerate()
        .map(|(index, page_id)| {
            let page = snapshot.page(page_id)?.page()?;
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
            Ok::<_, anyhow::Error>((page_id, stem))
        })
        .collect::<Result<Vec<_>>>()?;
    stream::iter(jobs)
        .map(|(page_id, stem)| {
            let renderer = renderer.clone();
            let rasterizer = Arc::clone(&rasterizer);
            let snapshot = snapshot.clone();
            let directory = directory.clone();
            async move {
                let frame = renderer.render(&snapshot, page_id).await?;
                match format {
                    ExportFormat::Png => {
                        let image =
                            rasterize(Arc::clone(&rasterizer), &frame, RasterOptions::default())
                                .await?
                                .image;
                        tokio::task::spawn_blocking(move || -> Result<()> {
                            let file =
                                std::fs::File::create(directory.join(format!("{stem}.png")))?;
                            PngEncoder::new_with_quality(
                                file,
                                CompressionType::Best,
                                FilterType::Adaptive,
                            )
                            .write_image(
                                image.as_raw(),
                                image.width(),
                                image.height(),
                                ExtendedColorType::Rgba8,
                            )?;
                            Ok(())
                        })
                        .await
                        .context("PNG export worker stopped unexpectedly")??;
                    }
                    ExportFormat::Psd => {
                        let bytes = export_page(
                            Arc::clone(&rasterizer),
                            &snapshot,
                            &frame,
                            &PsdExportOptions::default(),
                        )
                        .await?;
                        tokio::fs::write(directory.join(format!("{stem}.psd")), bytes).await?;
                    }
                }
                tracing::info!(
                    target: "koharu_metrics",
                    metric = "page_exported",
                    format = ?format,
                );
                Ok::<_, anyhow::Error>(())
            }
        })
        .buffer_unordered(4)
        .try_collect::<Vec<_>>()
        .await?;
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
    use super::*;

    #[test]
    fn omits_pages_with_only_empty_text() {
        assert!(text_export_page(2, vec![String::new(), "  ".to_owned()]).is_none());
    }

    #[test]
    fn preserves_empty_text_placeholders_on_non_empty_pages() {
        let page = text_export_page(
            1,
            vec!["first".to_owned(), String::new(), "third".to_owned()],
        )
        .expect("page contains text");

        assert_eq!(page.texts, vec!["first", "", "third"]);
    }

    #[test]
    fn serializes_empty_page_list_when_all_pages_are_empty() {
        let pages = [vec![String::new()], vec![" ".to_owned()]]
            .into_iter()
            .enumerate()
            .filter_map(|(index, texts)| text_export_page(index + 1, texts))
            .collect();
        let bytes = serialize_text_export(pages).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(value, serde_json::json!({ "pages": [] }));
    }

    #[test]
    fn extracts_json_from_llm_wrappers() {
        assert_eq!(
            extract_json("Here:\n```json\n{\"pages\":[]}\n```\nDone."),
            Some("{\"pages\":[]}")
        );
        assert_eq!(
            extract_json("Some text {\"pages\":[]} after"),
            Some("{\"pages\":[]}")
        );
        assert_eq!(extract_json("no JSON here"), None);
    }

    #[test]
    fn exported_texts_round_trip_through_import_format() {
        let exported = TextExport {
            pages: vec![
                TextExportPage {
                    page: 1,
                    texts: vec!["First translation".to_owned(), String::new()],
                },
                TextExportPage {
                    page: 3,
                    texts: vec!["Only text on page 3".to_owned()],
                },
            ],
        };

        let bytes = serialize_text_export(exported.pages.clone()).unwrap();
        let imported: TextExport = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(imported, exported);
    }
}
