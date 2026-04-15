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

    /// Reorder pages to match the given id sequence.
    /// Order 0 is reserved as the "unassigned"
    pub async fn reorder_pages(&self, ids: &[String]) -> Result<()> {
        let mut project = self.project.write().await;
        let page_ids: Vec<String> = project.pages.iter().map(|p| p.id.clone()).collect();
        for id in ids {
            if !page_ids.contains(id) {
                anyhow::bail!("Document not found: {id}");
            }
        }
        if ids.len() != page_ids.len() {
            anyhow::bail!(
                "Reorder list length ({}) does not match page count ({})",
                ids.len(),
                page_ids.len()
            );
        }
        for (order, id) in ids.iter().enumerate() {
            if let Some(page) = project.pages.iter_mut().find(|p| &p.id == id) {
                page.order = (order as u32) + 1;
            }
        }
        project.pages.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| natord::compare(&a.name, &b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
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
        let next_order = project
            .pages
            .iter()
            .map(|p| p.order)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let imported = pages.clone();
        project.pages.extend(pages);
        assign_missing_orders(&mut project.pages, next_order);
        project.pages.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| natord::compare(&a.name, &b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
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
            order: doc.order,
            has_segment: doc.segment.is_some(),
            has_inpainted: doc.inpainted.is_some(),
            has_rendered: doc.rendered.is_some(),
            has_brush_layer: doc.brush_layer.is_some(),
            text_block_count: doc.text_blocks.len(),
        })
        .collect();
    entries.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| natord::compare(&a.name, &b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
    entries
}

fn load_or_create_project(root: &Path) -> Result<Project> {
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "toml") {
                let content = std::fs::read_to_string(entry.path())?;
                let mut project: Project = toml::from_str(&content).context("parse project")?;
                if !project.pages.is_empty() && project.pages.iter().all(|p| p.order == 0) {
                    assign_missing_orders(&mut project.pages, 1);
                }
                return Ok(project);
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

/// Assign sequential order values to pages that have `order == 0` (unassigned), starting at `start`.
fn assign_missing_orders(pages: &mut [Document], start: u32) {
    let mut zero_indices: Vec<usize> = pages
        .iter()
        .enumerate()
        .filter(|(_, p)| p.order == 0)
        .map(|(i, _)| i)
        .collect();
    zero_indices.sort_by(|&a, &b| {
        natord::compare(&pages[a].name, &pages[b].name).then_with(|| pages[a].id.cmp(&pages[b].id))
    });
    for (offset, idx) in zero_indices.into_iter().enumerate() {
        pages[idx].order = start + offset as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_core::Document;

    fn make_page(id: &str, name: &str, order: u32) -> Document {
        Document {
            id: id.to_string(),
            name: name.to_string(),
            order,
            ..Default::default()
        }
    }

    // ── assign_missing_orders ───────────────────────────────────────

    #[test]
    fn assign_missing_orders_sets_sequential_values() {
        let mut pages = vec![
            make_page("c", "gamma", 0),
            make_page("a", "alpha", 0),
            make_page("b", "beta", 0),
        ];
        assign_missing_orders(&mut pages, 1);
        assert_eq!(pages.iter().find(|p| p.name == "alpha").unwrap().order, 1);
        assert_eq!(pages.iter().find(|p| p.name == "beta").unwrap().order, 2);
        assert_eq!(pages.iter().find(|p| p.name == "gamma").unwrap().order, 3);
    }

    #[test]
    fn assign_missing_orders_natural_sorts_numeric_names() {
        let mut pages = vec![
            make_page("10", "10", 0),
            make_page("1", "1", 0),
            make_page("2", "2", 0),
        ];
        assign_missing_orders(&mut pages, 1);
        assert_eq!(pages.iter().find(|p| p.name == "1").unwrap().order, 1);
        assert_eq!(pages.iter().find(|p| p.name == "2").unwrap().order, 2);
        assert_eq!(pages.iter().find(|p| p.name == "10").unwrap().order, 3);
    }

    #[test]
    fn assign_missing_orders_respects_start_offset() {
        let mut pages = vec![make_page("a", "alpha", 0), make_page("b", "beta", 0)];
        assign_missing_orders(&mut pages, 5);
        assert_eq!(pages.iter().find(|p| p.id == "a").unwrap().order, 5);
        assert_eq!(pages.iter().find(|p| p.id == "b").unwrap().order, 6);
    }

    #[test]
    fn assign_missing_orders_skips_non_zero() {
        let mut pages = vec![make_page("a", "alpha", 3), make_page("b", "beta", 0)];
        assign_missing_orders(&mut pages, 10);
        assert_eq!(pages.iter().find(|p| p.id == "a").unwrap().order, 3);
        assert_eq!(pages.iter().find(|p| p.id == "b").unwrap().order, 10);
    }

    #[test]
    fn assign_missing_orders_empty_slice_is_noop() {
        let mut pages = Vec::<Document>::new();
        assign_missing_orders(&mut pages, 0);
        assert!(pages.is_empty());
    }

    // ── list_documents ──────────────────────────────────────────────

    #[test]
    fn list_documents_sorts_by_order_then_name() {
        let project = Project {
            name: "test".to_string(),
            pages: vec![
                make_page("x", "gamma", 2),
                make_page("y", "alpha", 0),
                make_page("z", "beta", 1),
            ],
        };
        let result = list_documents(&project);
        assert_eq!(result[0].name, "alpha");
        assert_eq!(result[1].name, "beta");
        assert_eq!(result[2].name, "gamma");
    }

    #[test]
    fn list_documents_uses_name_as_tiebreaker() {
        let project = Project {
            name: "test".to_string(),
            pages: vec![make_page("b", "beta", 1), make_page("a", "alpha", 1)],
        };
        let result = list_documents(&project);
        assert_eq!(result[0].name, "alpha");
        assert_eq!(result[1].name, "beta");
    }

    #[test]
    fn list_documents_natural_sorts_numeric_and_mixed_numeric_names_on_ties() {
        let project = Project {
            name: "test".to_string(),
            pages: vec![
                make_page("a", "10", 1),
                make_page("b", "2", 1),
                make_page("c", "1", 1),
                make_page("d", "page 10", 1),
                make_page("e", "page 2", 1),
            ],
        };
        let result = list_documents(&project);
        let names: Vec<&str> = result.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["1", "2", "10", "page 2", "page 10"]);
    }

    // ── reorder_pages ───────────────────────────────────────────────

    fn open_test_storage(pages: Vec<Document>) -> (Storage, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let data_root = dir.path();
        let projects_root = data_root.join("projects");
        std::fs::create_dir_all(&projects_root).unwrap();
        let blobs_root = data_root.join("blobs");
        let blobs = BlobStore::new(blobs_root).unwrap();
        let project = Project {
            name: "test".to_string(),
            pages,
        };
        let content = toml::to_string_pretty(&project).unwrap();
        std::fs::write(projects_root.join("test.toml"), content).unwrap();
        let storage = Storage {
            images: ImageCache::new(blobs),
            project: RwLock::new(project),
            projects_root,
        };
        (storage, dir)
    }

    #[tokio::test]
    async fn reorder_pages_assigns_sequential_orders() {
        let (storage, _dir) = open_test_storage(vec![
            make_page("a", "alpha", 0),
            make_page("b", "beta", 1),
            make_page("c", "gamma", 2),
        ]);
        storage
            .reorder_pages(&["c".into(), "a".into(), "b".into()])
            .await
            .unwrap();
        let pages = storage.list_pages().await;
        assert_eq!(pages[0].id, "c");
        assert_eq!(pages[0].order, 1);
        assert_eq!(pages[1].id, "a");
        assert_eq!(pages[1].order, 2);
        assert_eq!(pages[2].id, "b");
        assert_eq!(pages[2].order, 3);
    }

    #[tokio::test]
    async fn reorder_pages_rejects_unknown_id() {
        let (storage, _dir) =
            open_test_storage(vec![make_page("a", "alpha", 0), make_page("b", "beta", 1)]);
        let err = storage
            .reorder_pages(&["a".into(), "unknown".into()])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Document not found"));
    }

    #[tokio::test]
    async fn reorder_pages_rejects_wrong_count() {
        let (storage, _dir) =
            open_test_storage(vec![make_page("a", "alpha", 0), make_page("b", "beta", 1)]);
        let err = storage.reorder_pages(&["a".into()]).await.unwrap_err();
        assert!(err.to_string().contains("does not match page count"));
    }

    #[tokio::test]
    async fn reorder_pages_persists_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let data_root = dir.path();
        let projects_root = data_root.join("projects");
        std::fs::create_dir_all(&projects_root).unwrap();
        let blobs_root = data_root.join("blobs");
        let blobs = BlobStore::new(&blobs_root).unwrap();
        let project = Project {
            name: "test".to_string(),
            pages: vec![make_page("a", "alpha", 0), make_page("b", "beta", 1)],
        };
        let content = toml::to_string_pretty(&project).unwrap();
        std::fs::write(projects_root.join("test.toml"), &content).unwrap();
        let storage = Storage {
            images: ImageCache::new(blobs),
            project: RwLock::new(project),
            projects_root: projects_root.clone(),
        };
        storage
            .reorder_pages(&["b".into(), "a".into()])
            .await
            .unwrap();
        // Re-load from disk
        let reloaded: Project =
            toml::from_str(&std::fs::read_to_string(projects_root.join("test.toml")).unwrap())
                .unwrap();
        assert_eq!(reloaded.pages[0].id, "b");
        assert_eq!(reloaded.pages[0].order, 1);
        assert_eq!(reloaded.pages[1].id, "a");
        assert_eq!(reloaded.pages[1].order, 2);
    }

    // ── load_or_create_project migration ────────────────────────────

    #[test]
    fn load_project_migrates_all_zero_orders() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            name: "test".to_string(),
            pages: vec![make_page("b", "beta", 0), make_page("a", "alpha", 0)],
        };
        let content = toml::to_string_pretty(&project).unwrap();
        std::fs::write(dir.path().join("test.toml"), content).unwrap();
        let loaded = load_or_create_project(dir.path()).unwrap();
        let alpha = loaded.pages.iter().find(|p| p.id == "a").unwrap();
        let beta = loaded.pages.iter().find(|p| p.id == "b").unwrap();
        assert_eq!(alpha.order, 1);
        assert_eq!(beta.order, 2);
    }

    #[test]
    fn load_project_does_not_migrate_if_any_nonzero() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            name: "test".to_string(),
            pages: vec![make_page("a", "alpha", 5), make_page("b", "beta", 0)],
        };
        let content = toml::to_string_pretty(&project).unwrap();
        std::fs::write(dir.path().join("test.toml"), content).unwrap();
        let loaded = load_or_create_project(dir.path()).unwrap();
        assert_eq!(loaded.pages.iter().find(|p| p.id == "a").unwrap().order, 5);
        assert_eq!(loaded.pages.iter().find(|p| p.id == "b").unwrap().order, 0);
    }

    // ── reorder + import interaction ────────────────────────────────

    #[tokio::test]
    async fn reorder_first_page_not_corrupted_by_new_import() {
        let (storage, _dir) =
            open_test_storage(vec![make_page("a", "alpha", 1), make_page("b", "beta", 2)]);

        // Reorder: B first, A second
        storage
            .reorder_pages(&["b".into(), "a".into()])
            .await
            .unwrap();

        let pages = storage.list_pages().await;
        assert_eq!(pages[0].id, "b");
        assert_eq!(pages[1].id, "a");

        {
            let mut project = storage.project.write().await;
            let next_order = project
                .pages
                .iter()
                .map(|p| p.order)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            project.pages.push(make_page("c", "charlie", 0));
            assign_missing_orders(&mut project.pages, next_order);
            project.pages.sort_by(|a, b| {
                a.order
                    .cmp(&b.order)
                    .then_with(|| a.name.cmp(&b.name))
                    .then_with(|| a.id.cmp(&b.id))
            });
        }

        let pages = storage.list_pages().await;
        assert_eq!(
            pages[0].id, "b",
            "reordered first page must keep position after import"
        );
        assert_eq!(
            pages[1].id, "a",
            "reordered second page must keep position after import"
        );
        assert_eq!(pages[2].id, "c", "new page must be appended at end");
    }

    #[tokio::test]
    async fn multiple_reorders_and_imports_preserve_order() {
        let (storage, _dir) =
            open_test_storage(vec![make_page("a", "alpha", 1), make_page("b", "beta", 2)]);

        // Reorder to [B, A]
        storage
            .reorder_pages(&["b".into(), "a".into()])
            .await
            .unwrap();
        let pages = storage.list_pages().await;
        assert_eq!(pages[0].id, "b");
        assert_eq!(pages[1].id, "a");

        // Add page C
        {
            let mut project = storage.project.write().await;
            let next_order = project
                .pages
                .iter()
                .map(|p| p.order)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            project.pages.push(make_page("c", "charlie", 0));
            assign_missing_orders(&mut project.pages, next_order);
            project.pages.sort_by(|a, b| {
                a.order
                    .cmp(&b.order)
                    .then_with(|| a.name.cmp(&b.name))
                    .then_with(|| a.id.cmp(&b.id))
            });
        }
        let pages = storage.list_pages().await;
        assert_eq!(pages[0].id, "b", "B must stay first after adding C");
        assert_eq!(pages[1].id, "a", "A must stay second after adding C");
        assert_eq!(pages[2].id, "c", "C must be third");

        // Reorder to [C, B, A]
        storage
            .reorder_pages(&["c".into(), "b".into(), "a".into()])
            .await
            .unwrap();
        let pages = storage.list_pages().await;
        assert_eq!(pages[0].id, "c");
        assert_eq!(pages[1].id, "b");
        assert_eq!(pages[2].id, "a");

        // Add page D
        {
            let mut project = storage.project.write().await;
            let next_order = project
                .pages
                .iter()
                .map(|p| p.order)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            project.pages.push(make_page("d", "delta", 0));
            assign_missing_orders(&mut project.pages, next_order);
            project.pages.sort_by(|a, b| {
                a.order
                    .cmp(&b.order)
                    .then_with(|| a.name.cmp(&b.name))
                    .then_with(|| a.id.cmp(&b.id))
            });
        }

        let pages = storage.list_pages().await;
        assert_eq!(pages[0].id, "c", "C must stay first after adding D");
        assert_eq!(pages[1].id, "b", "B must stay second after adding D");
        assert_eq!(pages[2].id, "a", "A must stay third after adding D");
        assert_eq!(pages[3].id, "d", "D must be last");
    }
}
