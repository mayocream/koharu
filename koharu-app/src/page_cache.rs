use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use image::DynamicImage;
use lru::LruCache;
use tokio::sync::RwLock;

use koharu_core::{Document, DocumentSummary, FileEntry};

use crate::blob_store::BlobStore;
use crate::manifest::{ManifestStore, PageManifest};

const DEFAULT_CAPACITY: usize = 5;

#[derive(Clone)]
pub struct PageCache {
    cache: Arc<RwLock<LruCache<String, Document>>>,
    pub(crate) blobs: BlobStore,
    manifests: ManifestStore,
}

impl PageCache {
    pub fn new(blobs: BlobStore, manifests: ManifestStore) -> Self {
        Self {
            cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(DEFAULT_CAPACITY).unwrap(),
            ))),
            blobs,
            manifests,
        }
    }

    /// Get a page, loading from disk if not cached.
    pub async fn get(&self, id: &str) -> Result<Document> {
        // Check cache first
        {
            let mut cache = self.cache.write().await;
            if let Some(doc) = cache.get(id) {
                return Ok(doc.clone());
            }
        }
        // Cache miss -- load from manifest + blobs
        let doc = self.load_from_disk(id)?;
        let mut cache = self.cache.write().await;
        cache.put(id.to_string(), doc.clone());
        Ok(doc)
    }

    /// Update a page in cache and save manifest + blobs to disk.
    pub async fn put(&self, doc: &Document) -> Result<()> {
        // Encode layers to blobs and save manifest
        self.save_to_disk(doc)?;
        // Update cache
        let mut cache = self.cache.write().await;
        cache.put(doc.id.clone(), doc.clone());
        Ok(())
    }

    /// Evict a page from cache (does not delete from disk).
    pub async fn evict(&self, id: &str) {
        let mut cache = self.cache.write().await;
        cache.pop(id);
    }

    /// Save a manifest without loading/putting the full document.
    pub fn save_manifest(&self, manifest: &PageManifest) -> Result<()> {
        self.manifests.save(manifest)
    }

    pub fn list_documents(&self) -> Result<Vec<DocumentSummary>> {
        let mut entries: Vec<DocumentSummary> = self
            .manifests
            .load_all()?
            .into_iter()
            .map(|manifest| DocumentSummary {
                id: manifest.id,
                name: manifest.name,
                width: manifest.width,
                height: manifest.height,
                has_segment: manifest.layers.segment.is_some(),
                has_inpainted: manifest.layers.inpainted.is_some(),
                has_rendered: manifest.layers.rendered.is_some(),
                has_brush_layer: manifest.layers.brush_layer.is_some(),
                text_block_count: manifest.text_blocks.len(),
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        Ok(entries)
    }

    pub async fn replace_manifests(&self, manifests: &[PageManifest]) -> Result<()> {
        self.manifests.replace_all(manifests)?;
        let mut cache = self.cache.write().await;
        cache.clear();
        Ok(())
    }

    pub fn import_files(&self, inputs: Vec<FileEntry>) -> Result<Vec<PageManifest>> {
        use image::GenericImageView;
        use rayon::iter::{IntoParallelIterator, ParallelIterator};

        let manifests: Result<Vec<PageManifest>> = inputs
            .into_par_iter()
            .map(|file| {
                let image = image::load_from_memory(&file.data)
                    .map_err(|error| anyhow::anyhow!("failed to decode {}: {error}", file.name))?;
                let (width, height) = image.dimensions();
                let id = blake3::hash(&file.data).to_hex().to_string();
                let source = self.blobs.put(&file.data)?;
                let name = Path::new(&file.name)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                Ok(PageManifest {
                    id,
                    source,
                    name,
                    width,
                    height,
                    layers: crate::manifest::LayerRefs::default(),
                    text_blocks: Vec::new(),
                })
            })
            .collect();

        let mut manifests = manifests?;
        manifests.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        Ok(manifests)
    }

    fn load_from_disk(&self, id: &str) -> Result<Document> {
        let manifest = self.manifests.load(id)?;

        let image_bytes = self.blobs
            .get(&manifest.source)
            .with_context(|| format!("Source blob not found: {}", manifest.source))?;
        let image =
            image::load_from_memory(&image_bytes).context("Failed to decode source image")?;

        // Load layers from blob store
        let segment = self.load_layer(&manifest.layers.segment)?;
        let inpainted = self.load_layer(&manifest.layers.inpainted)?;
        let rendered = self.load_layer(&manifest.layers.rendered)?;
        let brush_layer = self.load_layer(&manifest.layers.brush_layer)?;

        Ok(Document {
            id: manifest.id,
            path: PathBuf::from(&manifest.name),
            name: manifest.name,
            image: image.into(),
            width: manifest.width,
            height: manifest.height,
            text_blocks: manifest.text_blocks,
            segment: segment.map(Into::into),
            inpainted: inpainted.map(Into::into),
            rendered: rendered.map(Into::into),
            brush_layer: brush_layer.map(Into::into),
        })
    }

    fn save_to_disk(&self, doc: &Document) -> Result<PageManifest> {
        let encode = |img: &koharu_core::SerializableDynamicImage| -> Result<String> {
            let mut buf = std::io::Cursor::new(Vec::new());
            img.0
                .write_to(&mut buf, image::ImageFormat::WebP)
                .context("Failed to encode layer")?;
            self.blobs.put(&buf.into_inner())
        };

        let segment = doc.segment.as_ref().map(encode).transpose()?;
        let inpainted = doc.inpainted.as_ref().map(encode).transpose()?;
        let rendered = doc.rendered.as_ref().map(encode).transpose()?;
        let brush_layer = doc.brush_layer.as_ref().map(encode).transpose()?;
        let source = match self.manifests.load(&doc.id) {
            Ok(existing) if !existing.source.is_empty() => existing.source,
            _ => {
                let mut buf = std::io::Cursor::new(Vec::new());
                let format =
                    image::ImageFormat::from_path(&doc.path).unwrap_or(image::ImageFormat::Png);
                doc.image
                    .0
                    .write_to(&mut buf, format)
                    .context("Failed to encode source image")?;
                self.blobs.put(&buf.into_inner())?
            }
        };

        let manifest = PageManifest {
            id: doc.id.clone(),
            source,
            name: doc.name.clone(),
            width: doc.width,
            height: doc.height,
            layers: crate::manifest::LayerRefs {
                segment,
                inpainted,
                rendered,
                brush_layer,
            },
            text_blocks: doc.text_blocks.clone(),
        };

        self.manifests.save(&manifest)?;
        Ok(manifest)
    }

    fn load_layer(&self, hash: &Option<String>) -> Result<Option<DynamicImage>> {
        match hash {
            Some(h) if !h.is_empty() => {
                let bytes = self.blobs.get(h)?;
                let img = image::load_from_memory(&bytes)
                    .with_context(|| format!("Failed to decode layer blob {h}"))?;
                Ok(Some(img))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::PathBuf;

    use image::{DynamicImage, ImageFormat};
    use koharu_core::FileEntry;

    use super::PageCache;
    use crate::{blob_store::BlobStore, manifest::ManifestStore};

    fn cache() -> (tempfile::TempDir, PageCache) {
        let tempdir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::new(tempdir.path().join("blobs")).unwrap();
        let manifests = ManifestStore::new(tempdir.path().join("pages")).unwrap();
        (tempdir, PageCache::new(blobs, manifests))
    }

    #[test]
    fn import_files_stores_source_as_blob_hash() {
        let (_tempdir, cache) = cache();

        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(4, 5)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();

        let manifests = cache
            .import_files(vec![FileEntry {
                name: "page.png".to_string(),
                data: encoded.into_inner(),
            }])
            .unwrap();

        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].name, "page");
        assert!(!manifests[0].source.is_empty());
        assert!(cache.blobs.exists(&manifests[0].source));
    }

    #[test]
    fn load_from_disk_reads_source_blob() {
        let (_tempdir, cache) = cache();

        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(4, 5)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();

        let manifest = cache
            .import_files(vec![FileEntry {
                name: "page.png".to_string(),
                data: encoded.into_inner(),
            }])
            .unwrap()
            .pop()
            .unwrap();

        cache.save_manifest(&manifest).unwrap();
        let document = cache.load_from_disk(&manifest.id).unwrap();

        assert_eq!(document.name, "page");
        assert_eq!(document.width, 4);
        assert_eq!(document.height, 5);
    }
}
