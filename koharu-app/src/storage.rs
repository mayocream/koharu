use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use image::DynamicImage;
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
    pub fn load(&self, r: &BlobRef) -> Result<DynamicImage> {
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(img) = cache.get(r) {
                return Ok(img.clone());
            }
        }
        let bytes = self.blobs.get(r)?;
        let img = image::load_from_memory(&bytes)?;
        self.cache.lock().unwrap().put(r.clone(), img.clone());
        Ok(img)
    }

    /// Read raw blob bytes for a ref.
    pub fn load_bytes(&self, r: &BlobRef) -> Result<Vec<u8>> {
        self.blobs.get(r)
    }

    /// Encode a DynamicImage as WebP, store in blob store, cache it, return ref.
    pub fn store_webp(&self, img: &DynamicImage) -> Result<BlobRef> {
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::WebP)?;
        let r = self.blobs.put(&buf.into_inner())?;
        self.cache.lock().unwrap().put(r.clone(), img.clone());
        Ok(r)
    }

    /// Store raw bytes (e.g. an imported image in its original format).
    pub fn store_bytes(&self, data: &[u8]) -> Result<BlobRef> {
        self.blobs.put(data)
    }
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
