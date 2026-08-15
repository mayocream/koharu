use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
};

use anyhow::{Context as _, Result};
use koharu_desktop::{CanvasState, Desktop};
use koharu_scene::{AssetInput, AssetMetadata, AssetRole, At, PageDraft};
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Cef, Manager as _, State, WebviewWindow, ipc::Channel};
use walkdir::WalkDir;

use super::{
    ChannelExt as _, Error,
    agent::AgentState,
    canvas::{CanvasChannel, CanvasView},
    preferences::Preferences,
    processing::{Job, JobChannel, Processing},
    project::{
        CurrentProject, Page, PageSummary, Project, ProjectInfo, ProjectLibrary, ProjectSummary,
    },
};

#[derive(Clone, Debug, Serialize, Type)]
pub struct StartupState {
    pub preferences: Preferences,
    pub jobs: Vec<Job>,
    pub canvas: CanvasState,
}

#[derive(Clone, Debug, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PageImportSource {
    Files,
    Folder,
    Paths { paths: Vec<PathBuf> },
}

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "cbz"];
const MAX_ARCHIVE_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;

struct ImportedPage {
    name: String,
    bytes: Arc<[u8]>,
    format: image::ImageFormat,
    width: u32,
    height: u32,
}

pub(crate) struct Initialization {
    ready: tokio::sync::watch::Sender<bool>,
}

impl Default for Initialization {
    fn default() -> Self {
        let (ready, _) = tokio::sync::watch::channel(false);
        Self { ready }
    }
}

impl Initialization {
    pub(crate) fn ready(&self) {
        self.ready.send_replace(true);
    }

    async fn wait(&self) -> Result<()> {
        let mut ready = self.ready.subscribe();
        while !*ready.borrow_and_update() {
            ready
                .changed()
                .await
                .context("startup state closed before initialization completed")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct Download {
    #[specta(type = f64)]
    pub id: u64,
    pub state: DownloadState,
    pub name: Option<String>,
    #[specta(type = f64)]
    pub completed: u64,
    #[specta(type = f64)]
    pub total: u64,
    pub error: Option<String>,
}

#[derive(Default)]
pub(crate) struct DownloadChannel {
    pub(crate) channel: Mutex<Option<Channel<Download>>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Running,
    Finished,
    Failed,
}

#[derive(Clone, Debug, Default, Serialize, Type)]
pub struct ModelResources {
    #[specta(type = f64)]
    pub process_memory: u64,
    #[specta(type = f64)]
    pub system_memory: u64,
    pub process_cpu: f32,
    pub devices: Vec<DeviceResources>,
}

#[derive(Default)]
pub(crate) struct ResourceChannel {
    pub(crate) channel: Mutex<Option<Channel<ModelResources>>>,
}

#[derive(Default)]
pub(crate) struct ProjectChannel {
    pub(crate) channel: Mutex<Option<Channel<Option<ProjectInfo>>>>,
}

#[derive(Clone, Debug, Default, Serialize, Type)]
pub struct DeviceResources {
    pub name: String,
    pub selected: bool,
    #[specta(type = Option<f64>)]
    pub memory_budget: Option<u64>,
    #[specta(type = Option<f64>)]
    pub memory_used: Option<u64>,
    pub utilization: Option<f32>,
}

impl From<koharu_pipeline::ResourceSnapshot> for ModelResources {
    fn from(value: koharu_pipeline::ResourceSnapshot) -> Self {
        Self {
            process_memory: value.process_memory_bytes,
            system_memory: value.system_memory_bytes,
            process_cpu: value.process_cpu_percent,
            devices: value
                .devices
                .into_iter()
                .map(|device| DeviceResources {
                    name: device.name,
                    selected: device.selected,
                    memory_budget: device.memory_budget_bytes,
                    memory_used: device.memory_used_bytes,
                    utilization: device.utilization_percent,
                })
                .collect(),
        }
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn subscribe(
    handle: AppHandle<Cef>,
    on_canvas: Channel<CanvasState>,
    on_job: Channel<Job>,
    on_download: Channel<Download>,
    on_resources: Channel<ModelResources>,
    on_project: Channel<Option<ProjectInfo>>,
) -> std::result::Result<StartupState, Error> {
    handle.state::<Initialization>().wait().await?;

    *handle.state::<CanvasChannel>().channel.lock() = Some(on_canvas);
    *handle.state::<JobChannel>().channel.lock() = Some(on_job);
    *handle.state::<DownloadChannel>().channel.lock() = Some(on_download);
    *handle.state::<ResourceChannel>().channel.lock() = Some(on_resources);
    *handle.state::<ProjectChannel>().channel.lock() = Some(on_project);

    let canvas = handle
        .state::<Desktop>()
        .lock()
        .canvas_state(handle.state::<CanvasView>().fitted.load(Ordering::Acquire));
    let preferences = Preferences::load()?;
    Ok(StartupState {
        preferences,
        jobs: handle
            .state::<Processing>()
            .jobs
            .lock()
            .values()
            .cloned()
            .collect(),
        canvas,
    })
}

async fn replace_project(handle: &AppHandle<Cef>, opened: Project) -> Result<()> {
    let snapshot = opened.snapshot();
    let page = opened.active_page();
    let info = opened.info();

    handle.state::<AgentState>().reset().await;
    let processing = handle.state::<Processing>();
    for stop in processing.stops.lock().values() {
        stop.stop();
    }
    processing.stops.lock().clear();
    processing.jobs.lock().clear();

    let previous = {
        let current = handle.state::<CurrentProject>();
        let mut current = current.project.lock().await;
        current.replace(opened)
    };

    handle
        .state::<CanvasView>()
        .fitted
        .store(true, Ordering::Release);
    let desktop = handle.state::<Desktop>();
    desktop.show_page(&snapshot, page).await?;
    let canvas = desktop.lock().canvas_state(true);
    drop(previous);
    handle.state::<CanvasChannel>().channel.publish(canvas);
    handle.state::<ProjectChannel>().channel.publish(Some(info));
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_project(
    project: State<'_, CurrentProject>,
) -> std::result::Result<Option<ProjectInfo>, Error> {
    Ok(project.project.lock().await.as_ref().map(Project::info))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_pages(
    project: State<'_, CurrentProject>,
) -> std::result::Result<Vec<PageSummary>, Error> {
    let snapshot = project
        .project
        .lock()
        .await
        .as_ref()
        .context("no project is open")?
        .snapshot();
    Ok(Project::pages(&snapshot)?)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_page(
    project: State<'_, CurrentProject>,
) -> std::result::Result<Option<Page>, Error> {
    let current = {
        let project = project.project.lock().await;
        project
            .as_ref()
            .map(|project| (project.snapshot(), project.active_page()))
    };
    Ok(current
        .and_then(|(snapshot, page)| page.map(|page| (snapshot, page)))
        .map(|(snapshot, page)| Project::page(&snapshot, page))
        .transpose()?)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_projects(
    library: State<'_, ProjectLibrary>,
) -> std::result::Result<Vec<ProjectSummary>, Error> {
    Ok(library.list()?)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn create_project(
    name: String,
    handle: AppHandle<Cef>,
) -> std::result::Result<(), Error> {
    let library = handle.state::<ProjectLibrary>().inner().clone();
    let opened = library.create(&name).await?;
    replace_project(&handle, opened).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn open_project(
    name: String,
    handle: AppHandle<Cef>,
) -> std::result::Result<(), Error> {
    let library = handle.state::<ProjectLibrary>().inner().clone();
    let opened = library.open(&name).await?;
    replace_project(&handle, opened).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn close_project(handle: AppHandle<Cef>) -> std::result::Result<(), Error> {
    close_current_project(&handle).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_project(
    name: String,
    handle: AppHandle<Cef>,
) -> std::result::Result<(), Error> {
    let active = handle
        .state::<CurrentProject>()
        .project
        .lock()
        .await
        .as_ref()
        .is_some_and(|project| project.name == name);
    if active {
        close_current_project(&handle).await?;
    }
    let library = handle.state::<ProjectLibrary>().inner().clone();
    tokio::task::spawn_blocking(move || library.delete(&name))
        .await
        .context("project deletion worker stopped unexpectedly")??;
    Ok(())
}

async fn close_current_project(handle: &AppHandle<Cef>) -> Result<()> {
    handle.state::<AgentState>().reset().await;
    let processing = handle.state::<Processing>();
    for stop in processing.stops.lock().values() {
        stop.stop();
    }
    processing.stops.lock().clear();
    processing.jobs.lock().clear();
    let previous = {
        let current = handle.state::<CurrentProject>();
        let mut current = current.project.lock().await;
        current.take()
    };
    handle
        .state::<CanvasView>()
        .fitted
        .store(true, Ordering::Release);
    let desktop = handle.state::<Desktop>();
    desktop.clear().await;
    let result = desktop.lock().canvas_state(true);
    drop(previous);
    handle.state::<CanvasChannel>().channel.publish(result);
    handle.state::<ProjectChannel>().channel.publish(None);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn import_pages(
    source: PageImportSource,
    window: WebviewWindow<Cef>,
    desktop: State<'_, Desktop>,
    project: State<'_, CurrentProject>,
    canvas_view: State<'_, CanvasView>,
    processing: State<'_, Processing>,
    canvas_channel: State<'_, CanvasChannel>,
) -> std::result::Result<(), Error> {
    if !processing.stops.lock().is_empty() {
        return Err(anyhow::anyhow!("pages cannot be imported while processing is running").into());
    }
    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("Pages", &["png", "jpg", "jpeg", "webp", "zip", "cbz"])
        .set_parent(&window);
    let paths = match source {
        PageImportSource::Files => dialog.pick_files().await.map(|files| {
            files
                .into_iter()
                .map(|file| file.path().to_owned())
                .collect::<Vec<_>>()
        }),
        PageImportSource::Folder => dialog
            .pick_folder()
            .await
            .map(|folder| vec![folder.path().to_owned()]),
        PageImportSource::Paths { paths } => Some(paths),
    };
    let Some(paths) = paths else {
        return Ok(());
    };
    let pages = tokio::task::spawn_blocking(move || load_pages(paths))
        .await
        .context("page import worker stopped unexpectedly")??;

    let (commit, page) = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        let source = AssetRole::new("source")?;
        let patch = project.snapshot().patch(|edit| {
            for ImportedPage {
                name,
                bytes,
                format,
                width,
                height,
            } in pages
            {
                let page = edit.add_page(
                    PageDraft::new(name, f64::from(width), f64::from(height)),
                    At::End,
                )?;
                edit.set_asset(
                    page,
                    &source,
                    AssetInput::new(
                        bytes,
                        format.to_mime_type(),
                        AssetMetadata {
                            width: Some(width),
                            height: Some(height),
                            attributes: Default::default(),
                        },
                    ),
                )?;
            }
            Ok(())
        })?;
        let commit = project.session.commit(patch).await?;
        project.record(vec![commit.revision]);
        project.reconcile_page();
        let page = project.active_page();
        (commit, page)
    };
    if desktop.synchronize(&commit.snapshot, page, &commit).await? {
        canvas_view.fitted.store(true, Ordering::Release);
    }
    let canvas = desktop
        .lock()
        .canvas_state(canvas_view.fitted.load(Ordering::Acquire));
    canvas_channel.channel.publish(canvas);
    Ok(())
}

fn load_pages(paths: Vec<PathBuf>) -> Result<Vec<ImportedPage>> {
    let mut files = collect_import_files(paths);
    alphanumeric_sort::sort_slice_by_path_key(&mut files, PathBuf::as_path);
    let pages = files
        .into_par_iter()
        .map(|file| {
            if has_extension(&file, ARCHIVE_EXTENSIONS) {
                load_archive(&file)
            } else {
                load_image_file(&file).map(|page| vec![page])
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if pages.is_empty() {
        anyhow::bail!("no supported images were found in the selection");
    }
    Ok(pages)
}

fn collect_import_files(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .flat_map(|path| {
            if path.is_dir() {
                WalkDir::new(path)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(|entry| match entry {
                        Ok(entry) if entry.file_type().is_file() => Some(entry.into_path()),
                        Ok(_) => None,
                        Err(error) => {
                            tracing::warn!(%error, "could not inspect an import directory entry");
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![path]
            }
        })
        .filter(|path| {
            has_extension(path, IMAGE_EXTENSIONS) || has_extension(path, ARCHIVE_EXTENSIONS)
        })
        .collect()
}

fn load_image_file(path: &Path) -> Result<ImportedPage> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    load_image(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("page")
            .to_owned(),
        bytes,
        &path.display().to_string(),
    )
}

fn load_archive(path: &Path) -> Result<Vec<ImportedPage>> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open archive {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read archive {}", path.display()))?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("failed to inspect entry {index} in {}", path.display()))?;
        let name = entry.name().to_owned();
        if entry.is_file() && has_extension(Path::new(&name), IMAGE_EXTENSIONS) {
            entries.push((index, name));
        }
    }
    alphanumeric_sort::sort_slice_by_str_key(&mut entries, |(_, name)| name);

    let mut total_bytes = 0_u64;
    entries
        .into_iter()
        .map(|(index, name)| {
            let entry_label = format!("{} in {}", name, path.display());
            let mut entry = archive
                .by_index(index)
                .with_context(|| format!("failed to open {entry_label}"))?;
            if entry.size() > MAX_ARCHIVE_ENTRY_BYTES {
                anyhow::bail!("{entry_label} exceeds the 512 MiB page limit");
            }
            total_bytes = total_bytes
                .checked_add(entry.size())
                .context("archive contents are too large")?;
            if total_bytes > MAX_ARCHIVE_TOTAL_BYTES {
                anyhow::bail!("{} exceeds the 4 GiB import limit", path.display());
            }

            let mut bytes = Vec::with_capacity(usize::try_from(entry.size())?);
            entry
                .by_ref()
                .take(MAX_ARCHIVE_ENTRY_BYTES + 1)
                .read_to_end(&mut bytes)
                .with_context(|| format!("failed to extract {entry_label}"))?;
            if bytes.len() as u64 > MAX_ARCHIVE_ENTRY_BYTES {
                anyhow::bail!("{entry_label} exceeds the 512 MiB page limit");
            }
            load_image(
                Path::new(&name)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("page")
                    .to_owned(),
                bytes,
                &entry_label,
            )
        })
        .collect()
}

fn load_image(name: String, bytes: Vec<u8>, source: &str) -> Result<ImportedPage> {
    let format = image::guess_format(&bytes)
        .with_context(|| format!("failed to identify image format for {source}"))?;
    let (width, height) = image::ImageReader::with_format(Cursor::new(bytes.as_slice()), format)
        .into_dimensions()
        .with_context(|| format!("failed to read image dimensions for {source}"))?;
    Ok(ImportedPage {
        name,
        bytes: Arc::from(bytes),
        format,
        width,
        height,
    })
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

#[cfg(test)]
mod import_tests {
    use std::io::{Cursor, Write as _};

    use image::{DynamicImage, ImageFormat, RgbaImage};
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    #[test]
    fn imports_archive_images_in_natural_order() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("chapter.cbz");
        let mut archive = ZipWriter::new(fs::File::create(&archive_path).unwrap());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("chapter/page-10.png", options).unwrap();
        archive.write_all(&png(10, 1)).unwrap();
        archive.start_file("chapter/notes.txt", options).unwrap();
        archive.write_all(b"ignored").unwrap();
        archive.start_file("chapter/page-2.png", options).unwrap();
        archive.write_all(&png(2, 1)).unwrap();
        archive.finish().unwrap();

        let pages = load_pages(vec![archive_path]).unwrap();

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].name, "page-2.png");
        assert_eq!((pages[0].width, pages[0].height), (2, 1));
        assert_eq!(pages[1].name, "page-10.png");
        assert_eq!((pages[1].width, pages[1].height), (10, 1));
    }

    #[test]
    fn expands_dropped_folders_and_ignores_unsupported_files() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("page.png"), png(3, 4)).unwrap();
        fs::write(nested.join("notes.txt"), b"ignored").unwrap();

        let pages = load_pages(vec![directory.path().to_owned()]).unwrap();

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].name, "page.png");
        assert_eq!((pages[0].width, pages[0].height), (3, 4));
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::new(width, height))
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn select_page(
    desktop: State<'_, Desktop>,
    page: koharu_scene::EntityId,
    project: State<'_, CurrentProject>,
    canvas_view: State<'_, CanvasView>,
    canvas_channel: State<'_, CanvasChannel>,
) -> std::result::Result<(), Error> {
    let snapshot = {
        let mut project = project.project.lock().await;
        let project = project.as_mut().context("no project is open")?;
        project.select_page(page)?;
        project.snapshot()
    };
    desktop.show_page(&snapshot, Some(page)).await?;
    canvas_view.fitted.store(true, Ordering::Release);
    let canvas = desktop.lock().canvas_state(true);
    canvas_channel.channel.publish(canvas);
    Ok(())
}
