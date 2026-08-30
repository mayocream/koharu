use anyhow::{Context as _, Result};
use futures::{StreamExt as _, stream};
use image::{
    ExtendedColorType, ImageEncoder as _, Rgb, RgbImage, RgbaImage,
    codecs::{
        jpeg::JpegEncoder,
        png::{CompressionType, FilterType, PngEncoder},
    },
};
use koharu_pipeline::StopToken;
use koharu_psd::{PsdExportOptions, export_page};
use koharu_rasterizer::{Raster, RasterOptions, Rasterizer};
use koharu_renderer::{Frame, Renderer};
use koharu_scene::{AssetRole, EntityId, Snapshot};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    io::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{AppHandle, Cef, Manager as _, State, WebviewWindow, ipc::IpcResponse};

use super::{
    ChannelExt as _, Error,
    processing::{Job, JobChannel, JobId, JobKind, JobState, Processing},
    project::CurrentProject,
};
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
    Cbz(ImageEncoding),
}

/// How each page is encoded inside a CBZ archive. Every variant is also an
/// importable format, so an exported archive can be opened again as a project.
#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ImageEncoding {
    Png,
    Jpeg,
    Webp,
}

impl ImageEncoding {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }
}

/// Quality of the lossy page encodings, owned by export because nothing else
/// consumes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct ExportConfig {
    pub jpeg_quality: u8,
    pub webp_quality: u8,
}

impl Default for ExportConfig {
    fn default() -> Self {
        // Manga pages carry screentone and fine line work, which compress badly,
        // so these sit above the usual photographic defaults.
        Self {
            jpeg_quality: 90,
            webp_quality: 85,
        }
    }
}

impl ExportConfig {
    pub fn load() -> Result<koharu_config::Config<Self>> {
        koharu_config::load("export")
    }

    /// Reads the stored quality, clamped so a hand-edited config file cannot
    /// hand an out-of-range value to an encoder.
    fn quality(self, encoding: ImageEncoding) -> u8 {
        match encoding {
            ImageEncoding::Png => 100,
            ImageEncoding::Jpeg => self.jpeg_quality.clamp(1, 100),
            ImageEncoding::Webp => self.webp_quality.clamp(1, 100),
        }
    }
}

/// Where a single export run writes its output.
///
/// Page formats produce one file per page inside a chosen directory; archive
/// formats collect every page into one chosen file.
enum Destination {
    Directory(PathBuf),
    Archive(PathBuf),
}

/// Replaces characters that are invalid in common filesystem names.
fn sanitize_filename(name: &str) -> String {
    name.chars()
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
        .collect()
}

/// Output file stem for one page, without an extension.
///
/// Archive members follow the comic archive convention of `P001`, `P002`, and
/// are numbered by position and nothing else. A page label is not ordered --
/// pages can be reordered after import -- and usually already carries its own
/// number from the source file, so including it would both misorder the archive
/// and repeat the numbering. Loose files keep the label, where it is what makes
/// a file identifiable on disk.
fn page_stem(format: ExportFormat, index: usize, total: usize, label: &str) -> String {
    let number = index + 1;
    match format {
        ExportFormat::Cbz(_) => {
            // Widen past three digits for a project that needs it, so members
            // still sort correctly. Matches how PDF import numbers its pages.
            let width = total.to_string().len().max(3);
            format!("P{number:0width$}")
        }
        ExportFormat::Png | ExportFormat::Psd => {
            let name = label
                .trim()
                .trim_end_matches(|character: char| character == '.' || character.is_whitespace());
            let name = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
            let name = sanitize_filename(name);
            format!(
                "{number:04}_{}",
                if name.is_empty() { "page" } else { &name }
            )
        }
    }
}

/// Writes received members into a CBZ archive, in the order they arrive.
///
/// `ZipWriter` is a single sequential writer, so one task owns it and pages are
/// handed over through a channel rather than written concurrently.
fn write_cbz(
    path: &Path,
    receiver: &mut tokio::sync::mpsc::Receiver<(String, Vec<u8>)>,
) -> Result<()> {
    let file = std::fs::File::create(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    let mut archive = zip::ZipWriter::new(file);
    // Page images are already PNG-compressed; deflating them again costs time
    // for no meaningful size reduction.
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    while let Some((name, bytes)) = receiver.blocking_recv() {
        archive.start_file(name, options)?;
        archive.write_all(&bytes)?;
    }
    archive.finish()?;
    Ok(())
}

/// Composes an image over white.
///
/// JPEG has no alpha channel and its encoder rejects RGBA input, so transparency
/// has to be resolved first. Dropping the channel would darken transparent areas
/// toward black, so compose the way the PSD preview does.
fn flatten_onto_white(image: &RgbaImage) -> RgbImage {
    let mut flattened = RgbImage::new(image.width(), image.height());
    for (target, source) in flattened.pixels_mut().zip(image.pixels()) {
        let [red, green, blue, alpha] = source.0;
        let alpha = f32::from(alpha) / 255.0;
        let over = |channel: u8| f32::from(channel).mul_add(alpha, 255.0 * (1.0 - alpha)) as u8;
        *target = Rgb([over(red), over(green), over(blue)]);
    }
    flattened
}

async fn encode_page(
    image: RgbaImage,
    encoding: ImageEncoding,
    config: ExportConfig,
) -> Result<Vec<u8>> {
    let quality = config.quality(encoding);
    tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        match encoding {
            ImageEncoding::Png => {
                PngEncoder::new_with_quality(
                    &mut bytes,
                    CompressionType::Best,
                    FilterType::Adaptive,
                )
                .write_image(
                    image.as_raw(),
                    image.width(),
                    image.height(),
                    ExtendedColorType::Rgba8,
                )?;
            }
            ImageEncoding::Jpeg => {
                let flattened = flatten_onto_white(&image);
                JpegEncoder::new_with_quality(&mut bytes, quality).write_image(
                    flattened.as_raw(),
                    flattened.width(),
                    flattened.height(),
                    ExtendedColorType::Rgb8,
                )?;
            }
            ImageEncoding::Webp => {
                // libwebp keeps the alpha channel and takes a quality factor,
                // which the `image` encoder does not expose.
                bytes = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height())
                    .encode(f32::from(quality))
                    .to_vec();
            }
        }
        Ok(bytes)
    })
    .await
    .context("page encode worker stopped unexpectedly")?
}

/// Advances a running export job and publishes the update.
fn advance_export(handle: &AppHandle<Cef>, id: JobId, completed: usize) {
    let job = {
        let processing = handle.state::<Processing>();
        let mut jobs = processing.jobs.lock();
        jobs.get_mut(&id).map(|job| {
            job.completed = completed;
            job.clone()
        })
    };
    if let Some(job) = job {
        handle.state::<JobChannel>().channel.publish(job);
    }
}

/// Retires an export job in its terminal state and publishes the update.
fn finish_export(handle: &AppHandle<Cef>, id: JobId, state: JobState, error: Option<String>) {
    let processing = handle.state::<Processing>();
    processing.exports.lock().remove(&id);
    let job = processing.jobs.lock().remove(&id).map(|mut job| {
        job.state = state;
        job.error = error;
        job
    });
    if let Some(job) = job {
        handle.state::<JobChannel>().channel.publish(job);
    }
}

enum Outcome {
    Finished,
    Stopped,
}

#[allow(clippy::too_many_arguments)]
async fn run_export(
    handle: &AppHandle<Cef>,
    id: JobId,
    stop: &StopToken,
    format: ExportFormat,
    destination: Destination,
    snapshot: Snapshot,
    renderer: Renderer,
    rasterizer: Arc<Rasterizer>,
    jobs: Vec<(EntityId, String)>,
    config: ExportConfig,
) -> Result<Outcome> {
    // Renders one page and returns its output file name and encoded bytes. Both
    // destinations share this stage; only the write side differs.
    let render_page = move |(page_id, stem): (EntityId, String)| {
        let renderer = renderer.clone();
        let rasterizer = Arc::clone(&rasterizer);
        let snapshot = snapshot.clone();
        async move {
            let frame = renderer.render(&snapshot, page_id).await?;
            let output = match format {
                ExportFormat::Png => {
                    let image = rasterize(rasterizer, &frame, RasterOptions::default())
                        .await?
                        .image;
                    (
                        format!("{stem}.png"),
                        encode_page(image, ImageEncoding::Png, config).await?,
                    )
                }
                ExportFormat::Cbz(encoding) => {
                    let image = rasterize(rasterizer, &frame, RasterOptions::default())
                        .await?
                        .image;
                    (
                        format!("{stem}.{}", encoding.extension()),
                        encode_page(image, encoding, config).await?,
                    )
                }
                ExportFormat::Psd => {
                    let bytes =
                        export_page(rasterizer, &snapshot, &frame, &PsdExportOptions::default())
                            .await?;
                    (format!("{stem}.psd"), bytes)
                }
            };
            tracing::info!(
                target: "koharu_metrics",
                metric = "page_exported",
                format = ?format,
            );
            Ok::<_, anyhow::Error>(output)
        }
    };
    match destination {
        Destination::Directory(directory) => {
            let mut pages = stream::iter(jobs).map(render_page).buffer_unordered(4);
            let mut completed = 0_usize;
            while let Some(page) = pages.next().await {
                if stop.stopped() {
                    return Ok(Outcome::Stopped);
                }
                let (name, bytes) = page?;
                let path = directory.join(name);
                tokio::fs::write(&path, bytes)
                    .await
                    .with_context(|| format!("failed to write {}", path.display()))?;
                completed += 1;
                advance_export(handle, id, completed);
            }
            Ok(Outcome::Finished)
        }
        Destination::Archive(path) => {
            // `ZipWriter` is a single sequential writer, so pages are rendered
            // concurrently but handed over in page order to one owning task.
            let (sender, mut receiver) = tokio::sync::mpsc::channel::<(String, Vec<u8>)>(4);
            let archive_path = path.clone();
            let writer =
                tokio::task::spawn_blocking(move || write_cbz(&archive_path, &mut receiver));
            let mut outcome = Ok(Outcome::Finished);
            {
                let mut pages = stream::iter(jobs).map(render_page).buffered(4);
                let mut completed = 0_usize;
                while let Some(page) = pages.next().await {
                    if stop.stopped() {
                        outcome = Ok(Outcome::Stopped);
                        break;
                    }
                    match page {
                        Ok(member) => {
                            if sender.send(member).await.is_err() {
                                outcome = Err(anyhow::anyhow!("CBZ writer stopped unexpectedly"));
                                break;
                            }
                            completed += 1;
                            advance_export(handle, id, completed);
                        }
                        Err(error) => {
                            outcome = Err(error);
                            break;
                        }
                    }
                }
            }
            drop(sender);
            let written = writer.await.context("CBZ writer stopped unexpectedly")?;
            let outcome = match (outcome, written) {
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
                (Ok(outcome), Ok(())) => Ok(outcome),
            };
            if !matches!(outcome, Ok(Outcome::Finished)) {
                // A partial archive is unreadable; do not leave one behind.
                let _ = tokio::fs::remove_file(&path).await;
            }
            outcome
        }
    }
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "export",
    skip_all,
    fields(origin = "user", format = ?format),
)]
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn export_pages(
    handle: AppHandle<Cef>,
    window: WebviewWindow<Cef>,
    pages: Vec<EntityId>,
    format: ExportFormat,
    project: State<'_, CurrentProject>,
    desktop: State<'_, Desktop>,
    processing: State<'_, Processing>,
    job_channel: State<'_, JobChannel>,
) -> std::result::Result<Option<JobId>, Error> {
    let (snapshot, project_name) = {
        let project = project.project.lock().await;
        let project = project.as_ref().context("no project is open")?;
        (project.snapshot(), project.name.clone())
    };
    let pages = if pages.is_empty() {
        snapshot.pages().map(|page| page.id()).collect()
    } else {
        pages
    };
    if pages.is_empty() {
        return Err(anyhow::anyhow!("there are no pages to export").into());
    }
    let dialog = rfd::AsyncFileDialog::new().set_parent(&window);
    let destination = match format {
        ExportFormat::Png | ExportFormat::Psd => {
            let Some(directory) = dialog
                .pick_folder()
                .await
                .map(|directory| directory.path().to_owned())
            else {
                return Ok(None);
            };
            Destination::Directory(directory)
        }
        ExportFormat::Cbz(_) => {
            let name = sanitize_filename(project_name.trim());
            let name = if name.is_empty() { "export" } else { &name };
            let Some(path) = dialog
                .add_filter("Comic archive", &["cbz"])
                .set_file_name(format!("{name}.cbz"))
                .save_file()
                .await
                .map(|path| path.path().to_owned())
            else {
                return Ok(None);
            };
            Destination::Archive(path)
        }
    };
    let renderer = desktop.renderer();
    let rasterizer = desktop.rasterizer().await?;
    let config = {
        let config = ExportConfig::load()?;
        let value = config.read()?;
        *value
    };
    let total = pages.len();
    let jobs = pages
        .into_iter()
        .enumerate()
        .map(|(index, page_id)| {
            let page = snapshot.page(page_id)?.page()?;
            Ok::<_, anyhow::Error>((page_id, page_stem(format, index, total, &page.label)))
        })
        .collect::<Result<Vec<_>>>()?;

    let id = JobId::new();
    let stop = StopToken::default();
    processing.exports.lock().insert(id, stop.clone());
    let job = Job {
        id,
        kind: JobKind::Export,
        state: JobState::Running,
        completed: 0,
        total: jobs.len(),
        page: None,
        stage: None,
        model: None,
        error: None,
    };
    processing.jobs.lock().insert(id, job.clone());
    job_channel.channel.publish(job);

    let task_handle = handle.clone();
    drop(tokio::spawn(async move {
        let result = run_export(
            &task_handle,
            id,
            &stop,
            format,
            destination,
            snapshot,
            renderer,
            rasterizer,
            jobs,
            config,
        )
        .await;
        let (state, error) = match result {
            Ok(Outcome::Finished) => (JobState::Finished, None),
            Ok(Outcome::Stopped) => (JobState::Stopped, None),
            Err(error) => {
                tracing::error!(%error, "export failed");
                (JobState::Failed, Some(format!("{error:#}")))
            }
        };
        tracing::info!(
            target: "koharu_metrics",
            metric = "export_result",
            outcome = match state {
                JobState::Stopped => "stopped",
                JobState::Failed => "failed",
                _ => "completed",
            },
        );
        finish_export(&task_handle, id, state, error);
    }));
    Ok(Some(id))
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
    fn sanitize_filename_replaces_path_and_reserved_characters() {
        assert_eq!(sanitize_filename("ch1/page:2"), "ch1_page_2");
        assert_eq!(sanitize_filename(r#"a<b>c"d|e?f*g\h"#), "a_b_c_d_e_f_g_h");
        assert_eq!(sanitize_filename("plain name"), "plain name");
    }

    fn sample() -> RgbaImage {
        RgbaImage::from_fn(8, 8, |x, y| {
            // Half opaque, half fully transparent, so the JPEG matte is exercised.
            let alpha = if x < 4 { 255 } else { 0 };
            image::Rgba([10 * x as u8, 10 * y as u8, 200, alpha])
        })
    }

    #[tokio::test]
    async fn every_encoding_produces_its_own_importable_format() {
        for (encoding, expected) in [
            (ImageEncoding::Png, image::ImageFormat::Png),
            (ImageEncoding::Jpeg, image::ImageFormat::Jpeg),
            (ImageEncoding::Webp, image::ImageFormat::WebP),
        ] {
            let bytes = encode_page(sample(), encoding, ExportConfig::default())
                .await
                .expect("encode page");
            assert_eq!(
                image::guess_format(&bytes).expect("identify encoded page"),
                expected,
                "{encoding:?} produced the wrong container",
            );
            // Koharu must be able to import what it exports.
            image::load_from_memory(&bytes).expect("decode encoded page");
        }
    }

    #[test]
    fn jpeg_matte_composes_transparent_pixels_onto_white() {
        let flattened = flatten_onto_white(&sample());
        // Column 4 is fully transparent, so it must resolve to white rather than
        // to the pixel's own colour.
        assert_eq!(flattened.get_pixel(4, 0).0, [255, 255, 255]);
        // Column 0 is opaque and must be untouched.
        assert_eq!(flattened.get_pixel(0, 0).0, [0, 0, 200]);
    }

    #[test]
    fn quality_is_clamped_for_hand_edited_config() {
        let config = ExportConfig {
            jpeg_quality: 0,
            webp_quality: 200,
        };
        assert_eq!(config.quality(ImageEncoding::Jpeg), 1);
        assert_eq!(config.quality(ImageEncoding::Webp), 100);
    }

    #[test]
    fn archive_members_follow_the_comic_archive_convention() {
        let cbz = ExportFormat::Cbz(ImageEncoding::Jpeg);
        // A label that already carries its own number must not repeat it.
        assert_eq!(page_stem(cbz, 0, 20, "001.jpg"), "P001");
        assert_eq!(page_stem(cbz, 9, 20, "010.jpg"), "P010");
        // Nor should a label leak into the archive in any other form.
        assert_eq!(page_stem(cbz, 1, 20, "pages/page2.png"), "P002");
        assert_eq!(page_stem(cbz, 2, 20, ""), "P003");
    }

    #[test]
    fn archive_numbering_widens_for_large_projects() {
        let cbz = ExportFormat::Cbz(ImageEncoding::Png);
        // Three digits is the convention, but members must still sort.
        assert_eq!(page_stem(cbz, 0, 999, "a"), "P001");
        assert_eq!(page_stem(cbz, 0, 1000, "a"), "P0001");
        assert_eq!(page_stem(cbz, 1233, 1500, "a"), "P1234");
    }

    #[test]
    fn loose_files_keep_the_page_label() {
        assert_eq!(
            page_stem(ExportFormat::Png, 0, 3, "cover.png"),
            "0001_cover",
            "the label is what identifies a loose file on disk",
        );
        assert_eq!(
            page_stem(ExportFormat::Psd, 1, 3, "ch1/page.psd"),
            "0002_ch1_page"
        );
        assert_eq!(page_stem(ExportFormat::Png, 2, 3, "   "), "0003_page");
    }

    #[test]
    fn encodings_use_distinct_extensions() {
        assert_eq!(ImageEncoding::Png.extension(), "png");
        assert_eq!(ImageEncoding::Jpeg.extension(), "jpg");
        assert_eq!(ImageEncoding::Webp.extension(), "webp");
    }

    #[tokio::test]
    async fn write_cbz_preserves_member_order() {
        let members = ["0001_a.png", "0002_b.png", "0010_c.png"];
        let path = std::env::temp_dir().join(format!(
            "koharu-export-order-{}-{:?}.cbz",
            std::process::id(),
            std::thread::current().id()
        ));

        let (sender, mut receiver) = tokio::sync::mpsc::channel::<(String, Vec<u8>)>(4);
        let archive_path = path.clone();
        let writer = tokio::task::spawn_blocking(move || write_cbz(&archive_path, &mut receiver));
        for name in members {
            sender
                .send((name.to_owned(), b"payload".to_vec()))
                .await
                .expect("send member");
        }
        drop(sender);
        writer.await.expect("writer task").expect("write archive");

        let file = std::fs::File::open(&path).expect("open archive");
        let mut archive = zip::ZipArchive::new(file).expect("read archive");
        let names = (0..archive.len())
            .map(|index| archive.by_index(index).expect("member").name().to_owned())
            .collect::<Vec<_>>();
        drop(archive);
        std::fs::remove_file(&path).expect("remove archive");

        // Page order, not archive-internal sorting, is what readers rely on.
        assert_eq!(names, members);
    }

    #[tokio::test]
    async fn write_cbz_stores_members_uncompressed_and_intact() {
        let payload = b"page-bytes".to_vec();
        let path = std::env::temp_dir().join(format!(
            "koharu-export-stored-{}-{:?}.cbz",
            std::process::id(),
            std::thread::current().id()
        ));

        let (sender, mut receiver) = tokio::sync::mpsc::channel::<(String, Vec<u8>)>(4);
        let archive_path = path.clone();
        let writer = tokio::task::spawn_blocking(move || write_cbz(&archive_path, &mut receiver));
        sender
            .send(("0001_page.png".to_owned(), payload.clone()))
            .await
            .expect("send member");
        drop(sender);
        writer.await.expect("writer task").expect("write archive");

        let file = std::fs::File::open(&path).expect("open archive");
        let mut archive = zip::ZipArchive::new(file).expect("read archive");
        let (compression, bytes) = {
            let mut member = archive.by_index(0).expect("member");
            let compression = member.compression();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut member, &mut bytes).expect("read member");
            (compression, bytes)
        };
        drop(archive);
        std::fs::remove_file(&path).expect("remove archive");

        assert_eq!(compression, zip::CompressionMethod::Stored);
        assert_eq!(bytes, payload);
    }
}
