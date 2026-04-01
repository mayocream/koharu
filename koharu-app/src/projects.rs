use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use image::{GenericImageView, ImageFormat};
use koharu_core::commands::FileEntry;
use koharu_core::{
    Document, ProjectPageStages, ProjectPageState, ProjectPageSummary, ProjectSessionState,
    ProjectSummary, SerializableDynamicImage, TextBlock,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;
use uuid::Uuid;

const APP_DATA_DIR: &str = "Koharu";
const PROJECTS_DIR: &str = "projects";
const PROJECTS_STATE_FILE: &str = "projects_state.json";
const MANIFEST_FILE: &str = "project_manifest.json";

#[cfg(test)]
static TEST_APP_DATA_ROOT: once_cell::sync::Lazy<std::sync::Mutex<Option<PathBuf>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ProjectsStateDisk {
    last_open_project_id: Option<String>,
    recent_project_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectManifestDisk {
    id: String,
    name: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    current_document_id: Option<String>,
    pages: Vec<ProjectPageStateDisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPageStateDisk {
    summary: ProjectPageSummary,
    asset_rel_path: String,
    thumbnail_rel_path: String,
    asset_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPageDisk {
    id: String,
    name: String,
    width: u32,
    height: u32,
    revision: u64,
    asset_rel_path: String,
    asset_hash: String,
    text_blocks: Vec<TextBlock>,
    stages: ProjectPageStages,
}

#[derive(Debug, Clone)]
struct ImportedPage {
    bytes: Vec<u8>,
    ext: String,
    asset_hash: String,
    document: Document,
}

pub fn projects_root() -> Result<PathBuf> {
    Ok(app_data_root()?.join(PROJECTS_DIR))
}

pub fn list_projects(recent_only: bool) -> Result<Vec<ProjectSummary>> {
    if recent_only {
        let state = load_projects_state()?;
        let mut projects = Vec::new();
        for project_id in state.recent_project_ids {
            let Some(root) = find_project_root(&project_id)? else {
                continue;
            };
            projects.push(load_project_summary(&root)?);
        }
        return Ok(projects);
    }

    let root = projects_root()?;
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();
    for entry in fs::read_dir(&root)
        .with_context(|| format!("failed to read projects dir `{}`", root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join(MANIFEST_FILE);
        if !manifest_path.exists() {
            continue;
        }
        projects.push(load_project_summary(&entry.path())?);
    }
    projects.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
    Ok(projects)
}

pub fn load_last_project() -> Result<Option<ProjectSessionState>> {
    let state = load_projects_state()?;
    let Some(project_id) = state.last_open_project_id else {
        return Ok(None);
    };

    match open_project(&project_id) {
        Ok(session) => Ok(Some(session)),
        Err(err) => {
            tracing::warn!(?err, project_id, "failed to restore last project");
            let mut updated = load_projects_state()?;
            updated.last_open_project_id = None;
            updated.recent_project_ids.retain(|id| id != &project_id);
            save_projects_state(&updated)?;
            Ok(None)
        }
    }
}

pub fn open_project(project_id: &str) -> Result<ProjectSessionState> {
    let root = find_project_root(project_id)?
        .ok_or_else(|| anyhow::anyhow!("Project not found: {project_id}"))?;
    let mut session = load_session_from_root(root)?;
    if session.current_document_id.is_none() {
        session.current_document_id = session.pages.first().map(|page| page.summary.id.clone());
    }
    session.summary.current_document_id = session.current_document_id.clone();
    save_manifest(&session)?;
    touch_recent(&session.summary.id)?;
    Ok(session)
}

pub fn create_project(files: Vec<FileEntry>) -> Result<ProjectSessionState> {
    let imported = parse_imported_pages(files)?;
    anyhow::ensure!(!imported.is_empty(), "No files uploaded");

    let name = sanitize_project_name(&imported[0].document.name);
    let project_id = Uuid::new_v4().simple().to_string();
    let folder_name = format!("{name}--{project_id}");
    let root = projects_root()?.join(folder_name);
    ensure_project_layout(&root)?;

    let now = now_ms();
    let mut session = ProjectSessionState {
        root: root.clone(),
        summary: ProjectSummary {
            id: project_id.clone(),
            name: imported[0].document.name.clone(),
            page_count: imported.len(),
            updated_at_ms: now,
            current_document_id: None,
        },
        pages: Vec::new(),
        current_document_id: None,
        loaded_documents: HashMap::new(),
    };

    for imported_page in imported {
        let page = add_imported_page(&root, imported_page)?;
        session.pages.push(page);
    }

    session.current_document_id = session.pages.first().map(|page| page.summary.id.clone());
    session.summary.current_document_id = session.current_document_id.clone();
    session.summary.page_count = session.pages.len();
    session.summary.updated_at_ms = now;
    save_manifest(&session)?;
    touch_recent(&session.summary.id)?;
    Ok(session)
}

pub fn append_files(session: &mut ProjectSessionState, files: Vec<FileEntry>) -> Result<usize> {
    let imported = parse_imported_pages(files)?;
    anyhow::ensure!(!imported.is_empty(), "No files uploaded");
    ensure_project_layout(&session.root)?;

    for imported_page in imported {
        let page = add_imported_page(&session.root, imported_page)?;
        session.pages.push(page);
    }

    if session.current_document_id.is_none() {
        session.current_document_id = session.pages.first().map(|page| page.summary.id.clone());
    }
    session.summary.page_count = session.pages.len();
    session.summary.updated_at_ms = now_ms();
    session.summary.current_document_id = session.current_document_id.clone();
    save_manifest(session)?;
    touch_recent(&session.summary.id)?;
    Ok(session.pages.len())
}

pub fn load_document(session: &mut ProjectSessionState, document_id: &str) -> Result<Document> {
    if let Some(document) = session.loaded_documents.get(document_id) {
        return Ok(document.clone());
    }

    let page = session
        .pages
        .iter()
        .find(|page| page.summary.id == document_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Document not found: {document_id}"))?;
    let page_disk: ProjectPageDisk = read_json(&page_disk_path(&session.root, document_id))?;
    let asset_path = session.root.join(&page.asset_rel_path);
    let asset_bytes = fs::read(&asset_path)
        .with_context(|| format!("failed to read asset `{}`", asset_path.display()))?;
    let asset_image = image::load_from_memory(&asset_bytes)
        .with_context(|| format!("failed to decode asset `{}`", asset_path.display()))?;
    let (width, height) = asset_image.dimensions();

    let document = Document {
        id: page_disk.id.clone(),
        path: asset_path.clone(),
        name: page_disk.name,
        image: SerializableDynamicImage(asset_image),
        width,
        height,
        revision: page_disk.revision,
        text_blocks: page_disk.text_blocks,
        segment: read_optional_layer(&segment_layer_path(&session.root, document_id))?,
        inpainted: read_optional_layer(&inpainted_cache_path(&session.root, document_id))?,
        rendered: read_optional_layer(&rendered_cache_path(&session.root, document_id))?,
        brush_layer: read_optional_layer(&brush_layer_path(&session.root, document_id))?,
    };

    trim_cache(session, document_id);
    session
        .loaded_documents
        .insert(document_id.to_string(), document.clone());
    Ok(document)
}

pub fn save_document(session: &mut ProjectSessionState, document: &Document) -> Result<()> {
    let page = session
        .pages
        .iter_mut()
        .find(|page| page.summary.id == document.id)
        .ok_or_else(|| anyhow::anyhow!("Document not found: {}", document.id))?;

    page.summary = page_summary_from_document(document, page.summary.stages.clone());
    write_thumbnail(&session.root, page, document)?;
    write_page_disk(&session.root, page, document)?;
    write_optional_layer(&segment_layer_path(&session.root, &document.id), document.segment.as_ref())?;
    write_optional_layer(
        &brush_layer_path(&session.root, &document.id),
        document.brush_layer.as_ref(),
    )?;
    write_optional_layer(
        &inpainted_cache_path(&session.root, &document.id),
        document.inpainted.as_ref(),
    )?;
    write_optional_layer(
        &rendered_cache_path(&session.root, &document.id),
        document.rendered.as_ref(),
    )?;

    trim_cache(session, &document.id);
    session
        .loaded_documents
        .insert(document.id.clone(), document.clone());
    session.summary.page_count = session.pages.len();
    session.summary.updated_at_ms = now_ms();
    session.summary.current_document_id = session.current_document_id.clone();
    save_manifest(session)?;
    touch_recent(&session.summary.id)?;
    Ok(())
}

pub fn save_session(session: &ProjectSessionState) -> Result<()> {
    save_manifest(session)?;
    touch_recent(&session.summary.id)?;
    Ok(())
}

pub fn set_current_document(
    session: &mut ProjectSessionState,
    document_id: Option<String>,
) -> Result<()> {
    if let Some(document_id) = document_id.as_deref() {
        anyhow::ensure!(
            session.pages.iter().any(|page| page.summary.id == document_id),
            "Document not found: {document_id}"
        );
    }

    session.current_document_id = document_id;
    session.summary.current_document_id = session.current_document_id.clone();
    session.summary.updated_at_ms = now_ms();
    save_manifest(session)?;
    touch_recent(&session.summary.id)?;
    Ok(())
}

pub fn read_thumbnail(session: &ProjectSessionState, document_id: &str) -> Result<Vec<u8>> {
    let page = session
        .pages
        .iter()
        .find(|page| page.summary.id == document_id)
        .ok_or_else(|| anyhow::anyhow!("Document not found: {document_id}"))?;
    let path = session.root.join(&page.thumbnail_rel_path);
    fs::read(&path).with_context(|| format!("failed to read thumbnail `{}`", path.display()))
}

fn add_imported_page(root: &Path, imported: ImportedPage) -> Result<ProjectPageState> {
    let page_id = format!("page_{}", Uuid::new_v4().simple());
    let asset_rel_path = format!("assets/pages/{page_id}.{}", imported.ext);
    let thumbnail_rel_path = format!("assets/thumbs/{page_id}.webp");
    let asset_path = root.join(&asset_rel_path);
    write_binary_atomic(&asset_path, &imported.bytes)?;

    let mut document = imported.document;
    document.id = page_id.clone();
    document.path = asset_path.clone();
    document.prepare_for_store();

    let page = ProjectPageState {
        summary: page_summary_from_document(&document, ProjectPageStages::default()),
        asset_rel_path,
        thumbnail_rel_path,
        asset_hash: imported.asset_hash,
    };

    write_thumbnail(root, &page, &document)?;
    write_page_disk(root, &page, &document)?;
    Ok(page)
}

fn parse_imported_pages(files: Vec<FileEntry>) -> Result<Vec<ImportedPage>> {
    let mut imported = Vec::new();

    for file in files {
        let name = file.name.clone();
        let path = PathBuf::from(&name);
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let asset_hash = blake3::hash(&file.data).to_hex().to_string();
        let documents = Document::from_bytes(path, file.data.clone())
            .with_context(|| format!("failed to parse document `{name}`"))?;
        for document in documents {
            imported.push(ImportedPage {
                bytes: file.data.clone(),
                ext: ext.clone(),
                asset_hash: asset_hash.clone(),
                document,
            });
        }
    }

    imported.sort_by(|left, right| left.document.name.cmp(&right.document.name));
    Ok(imported)
}

fn page_summary_from_document(document: &Document, stages: ProjectPageStages) -> ProjectPageSummary {
    ProjectPageSummary {
        id: document.id.clone(),
        name: document.name.clone(),
        width: document.width,
        height: document.height,
        revision: document.revision,
        has_segment: document.segment.is_some(),
        has_inpainted: document.inpainted.is_some(),
        has_brush_layer: document.brush_layer.is_some(),
        has_rendered: document.rendered.is_some(),
        text_block_count: document.text_blocks.len(),
        stages,
    }
}

fn write_page_disk(root: &Path, page: &ProjectPageState, document: &Document) -> Result<()> {
    let path = page_disk_path(root, &page.summary.id);
    let mut text_blocks = document.text_blocks.clone();
    for block in &mut text_blocks {
        block.rendered = None;
    }

    write_json_atomic(
        &path,
        &ProjectPageDisk {
            id: document.id.clone(),
            name: document.name.clone(),
            width: document.width,
            height: document.height,
            revision: document.revision,
            asset_rel_path: page.asset_rel_path.clone(),
            asset_hash: page.asset_hash.clone(),
            text_blocks,
            stages: page.summary.stages.clone(),
        },
    )
}

fn write_thumbnail(root: &Path, page: &ProjectPageState, document: &Document) -> Result<()> {
    let source = document.rendered.as_ref().unwrap_or(&document.image);
    let thumbnail = source.thumbnail(200, 200);
    let bytes = encode_image(&thumbnail.into(), "webp")?;
    write_binary_atomic(&root.join(&page.thumbnail_rel_path), &bytes)
}

fn write_optional_layer(path: &Path, image: Option<&SerializableDynamicImage>) -> Result<()> {
    if let Some(image) = image {
        let bytes = encode_image(image, "webp")?;
        write_binary_atomic(path, &bytes)?;
    } else if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove stale layer `{}`", path.display()))?;
    }
    Ok(())
}

fn read_optional_layer(path: &Path) -> Result<Option<SerializableDynamicImage>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes =
        fs::read(path).with_context(|| format!("failed to read layer `{}`", path.display()))?;
    let image = image::load_from_memory(&bytes)
        .with_context(|| format!("failed to decode layer `{}`", path.display()))?;
    Ok(Some(SerializableDynamicImage(image)))
}

fn load_project_summary(root: &Path) -> Result<ProjectSummary> {
    let manifest: ProjectManifestDisk = read_json(&root.join(MANIFEST_FILE))?;
    Ok(ProjectSummary {
        id: manifest.id,
        name: manifest.name,
        page_count: manifest.pages.len(),
        updated_at_ms: manifest.updated_at_ms,
        current_document_id: manifest.current_document_id,
    })
}

fn load_session_from_root(root: PathBuf) -> Result<ProjectSessionState> {
    let manifest: ProjectManifestDisk = read_json(&root.join(MANIFEST_FILE))?;
    let pages = manifest
        .pages
        .into_iter()
        .map(|page| ProjectPageState {
            summary: page.summary,
            asset_rel_path: page.asset_rel_path,
            thumbnail_rel_path: page.thumbnail_rel_path,
            asset_hash: page.asset_hash,
        })
        .collect::<Vec<_>>();
    let current_document_id = manifest
        .current_document_id
        .filter(|document_id| pages.iter().any(|page| page.summary.id == *document_id))
        .or_else(|| pages.first().map(|page| page.summary.id.clone()));

    Ok(ProjectSessionState {
        root,
        summary: ProjectSummary {
            id: manifest.id,
            name: manifest.name,
            page_count: pages.len(),
            updated_at_ms: manifest.updated_at_ms,
            current_document_id: current_document_id.clone(),
        },
        pages,
        current_document_id,
        loaded_documents: HashMap::new(),
    })
}

fn save_manifest(session: &ProjectSessionState) -> Result<()> {
    let path = session.root.join(MANIFEST_FILE);
    let created_at_ms = if path.exists() {
        read_json::<ProjectManifestDisk>(&path)
            .map(|manifest| manifest.created_at_ms)
            .unwrap_or(session.summary.updated_at_ms)
    } else {
        session.summary.updated_at_ms
    };
    write_json_atomic(
        &path,
        &ProjectManifestDisk {
            id: session.summary.id.clone(),
            name: session.summary.name.clone(),
            created_at_ms,
            updated_at_ms: session.summary.updated_at_ms,
            current_document_id: session.current_document_id.clone(),
            pages: session
                .pages
                .iter()
                .map(|page| ProjectPageStateDisk {
                    summary: page.summary.clone(),
                    asset_rel_path: page.asset_rel_path.clone(),
                    thumbnail_rel_path: page.thumbnail_rel_path.clone(),
                    asset_hash: page.asset_hash.clone(),
                })
                .collect(),
        },
    )
}

fn trim_cache(session: &mut ProjectSessionState, keep_document_id: &str) {
    session
        .loaded_documents
        .retain(|document_id, _| document_id == keep_document_id);
}

fn find_project_root(project_id: &str) -> Result<Option<PathBuf>> {
    let root = projects_root()?;
    if !root.exists() {
        return Ok(None);
    }

    for entry in fs::read_dir(&root)
        .with_context(|| format!("failed to read projects dir `{}`", root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join(MANIFEST_FILE);
        if !manifest_path.exists() {
            continue;
        }
        let summary = load_project_summary(&entry.path())?;
        if summary.id == project_id {
            return Ok(Some(entry.path()));
        }
    }

    Ok(None)
}

fn load_projects_state() -> Result<ProjectsStateDisk> {
    let path = projects_state_path()?;
    if !path.exists() {
        return Ok(ProjectsStateDisk::default());
    }
    read_json(&path)
}

fn save_projects_state(state: &ProjectsStateDisk) -> Result<()> {
    write_json_atomic(&projects_state_path()?, state)
}

fn touch_recent(project_id: &str) -> Result<()> {
    let mut state = load_projects_state()?;
    state.last_open_project_id = Some(project_id.to_string());
    state.recent_project_ids.retain(|id| id != project_id);
    state.recent_project_ids.insert(0, project_id.to_string());
    state.recent_project_ids.truncate(20);
    save_projects_state(&state)
}

fn ensure_project_layout(root: &Path) -> Result<()> {
    for dir in [
        root.to_path_buf(),
        root.join("pages"),
        root.join("assets/pages"),
        root.join("assets/thumbs"),
        root.join("layers/segment"),
        root.join("layers/brush"),
        root.join("cache/inpainted"),
        root.join("cache/rendered"),
    ] {
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create dir `{}`", dir.display()))?;
    }
    Ok(())
}

fn projects_state_path() -> Result<PathBuf> {
    let app_root = app_data_root()?;
    fs::create_dir_all(&app_root)
        .with_context(|| format!("failed to create app data dir `{}`", app_root.display()))?;
    Ok(app_root.join(PROJECTS_STATE_FILE))
}

fn app_data_root() -> Result<PathBuf> {
    #[cfg(test)]
    {
        if let Some(path) = TEST_APP_DATA_ROOT
            .lock()
            .expect("test app data root lock poisoned")
            .clone()
        {
            return Ok(path);
        }
    }

    let base = dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| anyhow::anyhow!("failed to resolve local app data directory"))?;
    Ok(base.join(APP_DATA_DIR))
}

fn page_disk_path(root: &Path, page_id: &str) -> PathBuf {
    root.join("pages").join(format!("{page_id}.json"))
}

fn segment_layer_path(root: &Path, page_id: &str) -> PathBuf {
    root.join("layers/segment").join(format!("{page_id}.webp"))
}

fn brush_layer_path(root: &Path, page_id: &str) -> PathBuf {
    root.join("layers/brush").join(format!("{page_id}.webp"))
}

fn inpainted_cache_path(root: &Path, page_id: &str) -> PathBuf {
    root.join("cache/inpainted").join(format!("{page_id}.webp"))
}

fn rendered_cache_path(root: &Path, page_id: &str) -> PathBuf {
    root.join("cache/rendered").join(format!("{page_id}.webp"))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing parent for `{}`", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent dir `{}`", parent.display()))?;
    let content = serde_json::to_vec_pretty(value).context("failed to serialize JSON")?;
    write_binary_atomic(path, &content)
}

fn write_binary_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing parent for `{}`", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent dir `{}`", parent.display()))?;
    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to stage `{}`", path.display()))?;
    temp.write_all(bytes)
        .with_context(|| format!("failed to write temp file for `{}`", path.display()))?;
    temp.flush()
        .with_context(|| format!("failed to flush temp file for `{}`", path.display()))?;

    match temp.persist(path) {
        Ok(_) => Ok(()),
        Err(err) => {
            if err.error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(anyhow::anyhow!(
                    "failed to persist file `{}`: {}",
                    path.display(),
                    err.error
                ));
            }

            if path.exists() {
                fs::remove_file(path).with_context(|| {
                    format!("failed to replace existing file `{}`", path.display())
                })?;
            }
            err.file.persist(path).map(|_| ()).map_err(|persist_err| {
                anyhow::anyhow!(
                    "failed to persist file `{}`: {}",
                    path.display(),
                    persist_err.error
                )
            })
        }
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse `{}`", path.display()))
}

fn encode_image(image: &SerializableDynamicImage, ext: &str) -> Result<Vec<u8>> {
    let format = ImageFormat::from_extension(ext).unwrap_or(ImageFormat::WebP);
    let mut cursor = std::io::Cursor::new(Vec::new());
    image
        .0
        .write_to(&mut cursor, format)
        .with_context(|| format!("failed to encode image as {ext}"))?;
    Ok(cursor.into_inner())
}

fn sanitize_project_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else if ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .if_empty_then("project")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

trait IfEmptyThen {
    fn if_empty_then(self, fallback: &str) -> String;
}

impl IfEmptyThen for String {
    fn if_empty_then(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

#[cfg(test)]
pub(crate) fn with_test_app_data_root<T>(run: impl FnOnce() -> T) -> T {
    static TEST_LOCK: once_cell::sync::Lazy<std::sync::Mutex<()>> =
        once_cell::sync::Lazy::new(|| std::sync::Mutex::new(()));

    struct TestRootGuard {
        _temp_dir: tempfile::TempDir,
    }

    impl Drop for TestRootGuard {
        fn drop(&mut self) {
            *TEST_APP_DATA_ROOT
                .lock()
                .expect("test app data root lock poisoned") = None;
        }
    }

    let _lock = TEST_LOCK.lock().expect("test root lock poisoned");
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    *TEST_APP_DATA_ROOT
        .lock()
        .expect("test app data root lock poisoned") = Some(temp_dir.path().join(APP_DATA_DIR));
    let _guard = TestRootGuard {
        _temp_dir: temp_dir,
    };
    run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use koharu_core::TextBlock;
    use std::io::Cursor;

    fn sample_file_entry(name: &str) -> FileEntry {
        let image = RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255]));
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .expect("png encode should work");
        FileEntry {
            name: name.to_string(),
            data: cursor.into_inner(),
        }
    }

    #[test]
    fn create_project_restores_last_project() {
        with_test_app_data_root(|| {
            let session =
                create_project(vec![sample_file_entry("page.png")]).expect("project should exist");
            let document_id = session
                .current_document_id
                .clone()
                .expect("project should have a current page");

            assert!(session.root.join(MANIFEST_FILE).exists());
            assert!(page_disk_path(&session.root, &document_id).exists());

            let reopened = open_project(&session.summary.id).expect("project should reopen");
            assert_eq!(reopened.summary.id, session.summary.id);
            assert_eq!(reopened.current_document_id.as_deref(), Some(document_id.as_str()));

            let restored = load_last_project()
                .expect("restore should succeed")
                .expect("project should restore");
            assert_eq!(restored.summary.id, session.summary.id);
            assert_eq!(restored.current_document_id.as_deref(), Some(document_id.as_str()));
        });
    }

    #[test]
    fn save_document_round_trips_layout_seed_and_lock() {
        with_test_app_data_root(|| {
            let mut session =
                create_project(vec![sample_file_entry("page.png")]).expect("project should exist");
            let document_id = session
                .current_document_id
                .clone()
                .expect("project should have a current page");

            let mut document =
                load_document(&mut session, &document_id).expect("document should load");
            let mut block = TextBlock {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
                text: Some("jp".to_string()),
                translation: Some("tr".to_string()),
                lock_layout_box: true,
                ..Default::default()
            };
            block.set_layout_seed(11.0, 22.0, 33.0, 44.0);
            document.text_blocks.push(block);
            document.prepare_for_store();

            save_document(&mut session, &document).expect("document should save");

            let mut reopened = open_project(&session.summary.id).expect("project should reopen");
            let loaded = load_document(&mut reopened, &document_id).expect("document should load");
            let block = loaded
                .text_blocks
                .first()
                .expect("text block should persist");

            assert!(block.lock_layout_box);
            assert_eq!(block.layout_seed_x, Some(11.0));
            assert_eq!(block.layout_seed_y, Some(22.0));
            assert_eq!(block.layout_seed_width, Some(33.0));
            assert_eq!(block.layout_seed_height, Some(44.0));
            assert_eq!(block.translation.as_deref(), Some("tr"));
        });
    }
}
