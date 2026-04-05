use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use image::{DynamicImage, RgbaImage};
use lru::LruCache;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use koharu_core::{BlobRef, Document, DocumentSummary};

const IMAGE_CACHE_CAPACITY: usize = 64;

// ── Blob Store ──────────────────────────────────────────────────────

struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Write bytes to the store, return the blake3 hash as a `BlobRef`.
    fn put(&self, data: &[u8]) -> Result<BlobRef> {
        let hash = blake3::hash(data).to_hex().to_string();
        let path = self.blob_path(&hash);
        if path.exists() {
            return Ok(BlobRef::new(hash));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data).with_context(|| format!("Failed to write blob {hash}"))?;
        Ok(BlobRef::new(hash))
    }

    /// Read bytes from the store by `BlobRef`.
    fn get(&self, r: &BlobRef) -> Result<Vec<u8>> {
        let hash = r.hash();
        let path = self.blob_path(hash);
        std::fs::read(&path).with_context(|| format!("Blob not found: {hash}"))
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        let (prefix, rest) = hash.split_at(2.min(hash.len()));
        self.root.join(prefix).join(rest)
    }
}

// ── Image Cache ─────────────────────────────────────────────────────

pub struct ImageCache {
    cache: Mutex<LruCache<BlobRef, DynamicImage>>,
    blobs: BlobStore,
}

impl ImageCache {
    fn new(blobs: BlobStore) -> Self {
        Self {
            cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(IMAGE_CACHE_CAPACITY).unwrap(),
            )),
            blobs,
        }
    }

    /// Load a decoded image, using cache. Returns cloned DynamicImage.
    #[tracing::instrument(level = "info", skip(self))]
    pub fn load(&self, r: &BlobRef) -> Result<DynamicImage> {
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(img) = cache.get(r) {
                return Ok(img.clone());
            }
        }
        let bytes = self.blobs.get(r)?;
        let img = decode_blob(&bytes)?;
        self.cache.lock().unwrap().put(r.clone(), img.clone());
        Ok(img)
    }

    /// Read raw blob bytes for a ref.
    pub fn load_bytes(&self, r: &BlobRef) -> Result<Vec<u8>> {
        self.blobs.get(r)
    }

    /// Encode a DynamicImage as WebP, store in blob store, cache it, return ref.
    #[tracing::instrument(level = "info", skip(self, img))]
    pub fn store_webp(&self, img: &DynamicImage) -> Result<BlobRef> {
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::WebP)?;
        let r = self.blobs.put(&buf.into_inner())?;
        self.cache.lock().unwrap().put(r.clone(), img.clone());
        Ok(r)
    }

    /// Store a DynamicImage as raw RGBA bytes with a 12-byte header.
    /// Near-zero encoding cost compared to WebP/PNG.
    #[tracing::instrument(level = "info", skip(self, img))]
    pub fn store_raw(&self, img: &DynamicImage) -> Result<BlobRef> {
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let pixels = rgba.as_raw();
        let mut buf = Vec::with_capacity(12 + pixels.len());
        buf.extend_from_slice(b"RGBA");
        buf.extend_from_slice(&w.to_le_bytes());
        buf.extend_from_slice(&h.to_le_bytes());
        buf.extend_from_slice(pixels);
        let r = self.blobs.put(&buf)?;
        self.cache.lock().unwrap().put(r.clone(), img.clone());
        Ok(r)
    }

    /// Store raw bytes (e.g. an imported image in its original format).
    pub fn store_bytes(&self, data: &[u8]) -> Result<BlobRef> {
        self.blobs.put(data)
    }

    /// Check if a blob is in our raw RGBA format (vs a standard image format).
    pub fn is_raw_rgba(&self, r: &BlobRef) -> bool {
        self.blobs
            .get(r)
            .map(|bytes| bytes.len() >= 4 && &bytes[..4] == b"RGBA")
            .unwrap_or(false)
    }
}

/// Decode a blob: raw RGBA (our format) or standard image format.
fn decode_blob(bytes: &[u8]) -> Result<DynamicImage> {
    if bytes.len() >= 12 && &bytes[..4] == b"RGBA" {
        let w = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let h = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let pixels = bytes[12..].to_vec();
        let img = RgbaImage::from_raw(w, h, pixels).context("invalid raw RGBA blob dimensions")?;
        return Ok(DynamicImage::ImageRgba8(img));
    }
    Ok(image::load_from_memory(bytes)?)
}

// ── Project ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub pages: Vec<Document>,
}

// ── Storage ─────────────────────────────────────────────────────────

/// Unified storage: blob-backed images with LRU cache, plus project metadata.
pub struct Storage {
    pub images: ImageCache,
    project: RwLock<Project>,
    projects_root: PathBuf,
}

impl Storage {
    pub fn open(data_root: &Path) -> Result<Self> {
        let blobs_root = data_root.join("blobs");
        let projects_root = data_root.join("projects");
        std::fs::create_dir_all(&projects_root)?;

        let blobs = BlobStore::new(blobs_root)?;
        let project = load_or_create_project(&projects_root)?;

        Ok(Self {
            images: ImageCache::new(blobs),
            project: RwLock::new(project),
            projects_root,
        })
    }

    /// Get a clone of a page.
    pub async fn page(&self, id: &str) -> Result<Document> {
        self.project
            .read()
            .await
            .pages
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Document not found: {id}"))
    }

    /// List all pages as summaries.
    pub async fn list_pages(&self) -> Vec<DocumentSummary> {
        list_documents(&*self.project.read().await)
    }

    /// Get the total number of pages.
    pub async fn page_count(&self) -> usize {
        self.project.read().await.pages.len()
    }

    /// Collect all page ids.
    pub async fn page_ids(&self) -> Vec<String> {
        self.project
            .read()
            .await
            .pages
            .iter()
            .map(|p| p.id.clone())
            .collect()
    }

    /// Read-lock the project and run a closure.
    pub async fn with_project<R>(&self, f: impl FnOnce(&Project) -> R) -> R {
        let project = self.project.read().await;
        f(&project)
    }

    /// Update a page in-place and auto-save the project.
    #[tracing::instrument(level = "info", skip(self, f))]
    pub async fn update_page(&self, id: &str, f: impl FnOnce(&mut Document)) -> Result<()> {
        let mut project = self.project.write().await;
        let page = project
            .pages
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow::anyhow!("Document not found: {id}"))?;
        f(page);
        self.persist(&project)
    }

    /// Replace a page entirely and auto-save the project.
    pub async fn save_page(&self, id: &str, page: Document) -> Result<()> {
        let mut project = self.project.write().await;
        if let Some(existing) = project.pages.iter_mut().find(|p| p.id == id) {
            *existing = page;
        }
        self.persist(&project)
    }

    /// Import files, create pages, save project.
    pub async fn import_files(
        &self,
        files: Vec<koharu_core::FileEntry>,
        replace: bool,
    ) -> Result<Vec<Document>> {
        use rayon::iter::{IntoParallelIterator, ParallelIterator};

        let pages: Vec<Document> = files
            .into_par_iter()
            .filter_map(|file| {
                let reader = image::ImageReader::new(std::io::Cursor::new(&file.data))
                    .with_guessed_format()
                    .ok()?;
                let (width, height) = reader.into_dimensions().ok()?;
                let id = blake3::hash(&file.data).to_hex().to_string();
                let source = self.images.store_bytes(&file.data).ok()?;
                let name = Path::new(&file.name)
                    .file_stem()?
                    .to_string_lossy()
                    .to_string();
                Some(Document {
                    id,
                    name,
                    width,
                    height,
                    source,
                    ..Default::default()
                })
            })
            .collect();

        let mut project = self.project.write().await;
        if replace {
            project.pages.clear();
        }
        let imported = pages.clone();
        project.pages.extend(pages);
        project
            .pages
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        self.persist(&project)?;
        Ok(imported)
    }

    fn persist(&self, project: &Project) -> Result<()> {
        let path = self.projects_root.join(format!("{}.toml", project.name));
        let content = toml::to_string_pretty(project).context("serialize project")?;
        std::fs::write(&path, content).context("write project")
    }

    /// Export the project and all referenced images to a .khr file (zip format)
    pub async fn export_khr(&self, output_path: &Path) -> Result<()> {
        let project = self.project.read().await;

        let file = std::fs::File::create(output_path)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // 1. Write project.toml
        let project_toml = toml::to_string_pretty(&*project).context("serialize project")?;
        zip.start_file("project.toml", options.clone())?;
        use std::io::Write;
        zip.write_all(project_toml.as_bytes())?;

        // 2. Gather all image blobs used by pages
        let mut blobs = std::collections::HashSet::new();
        for page in &project.pages {
            blobs.insert(page.source.clone());
            if let Some(r) = &page.segment { blobs.insert(r.clone()); }
            if let Some(r) = &page.inpainted { blobs.insert(r.clone()); }
            if let Some(r) = &page.rendered { blobs.insert(r.clone()); }
            if let Some(r) = &page.brush_layer { blobs.insert(r.clone()); }
            for block in &page.text_blocks {
                if let Some(r) = &block.rendered { blobs.insert(r.clone()); }
            }
        }

        // 3. Write blobs into "blobs/" directory
        for blob in blobs {
            if blob.is_empty() { continue; }
            if let Ok(bytes) = self.images.load_bytes(&blob) {
                zip.start_file(format!("blobs/{}", blob.hash()), options.clone())?;
                zip.write_all(&bytes)?;
            }
        }

        zip.finish()?;

        Ok(())
    }

    /// Import a project from a .khr file
    pub async fn import_khr(&self, input_path: &Path) -> Result<()> {
        let file = std::fs::File::open(input_path)?;
        let mut zip = zip::ZipArchive::new(file)?;

        // 1. Read project.toml
        let mut project_toml = String::new();
        {
            let mut project_file = zip.by_name("project.toml").context("Missing project.toml in archive")?;
            use std::io::Read;
            project_file.read_to_string(&mut project_toml)?;
        }

        let new_project: Project = toml::from_str(&project_toml).context("Invalid project.toml format")?;

        // 2. Extract blobs
        let mut i = 0;
        while i < zip.len() {
            let mut file = zip.by_index(i)?;
            if file.name().starts_with("blobs/") && file.is_file() {
                use std::io::Read;
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer)?;
                // Insert directly into blob store
                let _ = self.images.store_bytes(&buffer);
            }
            i += 1;
        }

        // 3. Update active project and persist
        {
            let mut current = self.project.write().await;
            *current = new_project;
            self.persist(&*current)?;
        }

        Ok(())
    }
}

fn list_documents(project: &Project) -> Vec<DocumentSummary> {
    let mut entries: Vec<DocumentSummary> = project
        .pages
        .iter()
        .map(|doc| DocumentSummary {
            id: doc.id.clone(),
            name: doc.name.clone(),
            width: doc.width,
            height: doc.height,
            has_segment: doc.segment.is_some(),
            has_inpainted: doc.inpainted.is_some(),
            has_rendered: doc.rendered.is_some(),
            has_brush_layer: doc.brush_layer.is_some(),
            text_block_count: doc.text_blocks.len(),
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    entries
}

fn load_or_create_project(root: &Path) -> Result<Project> {
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "toml") {
                let content = std::fs::read_to_string(entry.path())?;
                return toml::from_str(&content).context("parse project");
            }
        }
    }
    let name = petname::petname(2, "-").unwrap_or_else(|| "untitled".to_string());
    let project = Project {
        name,
        pages: Vec::new(),
    };
    let content = toml::to_string_pretty(&project)?;
    std::fs::write(root.join(format!("{}.toml", project.name)), content)?;
    Ok(project)
}
