//! Content-addressed blob references and their project-local storage.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::DynamicImage;
use lru::LruCache;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const IMAGE_CACHE_CAPACITY: usize = 64;

/// Hex-encoded blake3 hash of an immutable blob.
#[derive(
    Default, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, ToSchema,
)]
#[serde(transparent)]
pub struct BlobRef(pub String);

impl BlobRef {
    pub fn new(hash: impl Into<String>) -> Self {
        Self(hash.into())
    }

    pub fn hash(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for BlobRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Content-addressed blob store with an LRU of decoded images.
pub struct BlobStore {
    root: PathBuf,
    cache: Mutex<LruCache<BlobRef, DynamicImage>>,
}

impl BlobStore {
    /// Open or create the store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .with_context(|| format!("create blob root {}", root.display()))?;
        Ok(Self {
            root,
            cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(IMAGE_CACHE_CAPACITY).expect("cache capacity nonzero"),
            )),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Decode image bytes and store their canonical, lossless WebP encoding.
    pub fn put_image_bytes(&self, data: &[u8]) -> Result<BlobRef> {
        let image = image::load_from_memory(data).context("decode image blob")?;
        self.put_webp(&image)
    }

    fn store_webp(&self, data: &[u8]) -> Result<BlobRef> {
        debug_assert!(is_webp(data));
        let hash = blake3::hash(data).to_hex().to_string();
        let path = self.blob_path(&hash);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, data).with_context(|| format!("write blob {hash}"))?;
        }
        Ok(BlobRef::new(hash))
    }

    pub fn get_bytes(&self, blob: &BlobRef) -> Result<Vec<u8>> {
        let path = self.blob_path(blob.hash());
        std::fs::read(&path).with_context(|| format!("blob not found: {}", blob.hash()))
    }

    pub fn exists(&self, blob: &BlobRef) -> bool {
        self.blob_path(blob.hash()).exists()
    }

    pub fn load_image(&self, blob: &BlobRef) -> Result<DynamicImage> {
        if let Some(image) = self.cache.lock().get(blob).cloned() {
            return Ok(image);
        }
        let bytes = self.get_bytes(blob)?;
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
            .context("decode stored WebP blob")?;
        self.cache.lock().put(blob.clone(), image.clone());
        Ok(image)
    }

    pub fn put_webp(&self, image: &DynamicImage) -> Result<BlobRef> {
        let encoded = encode_webp(image)?;
        let blob = self.store_webp(&encoded)?;
        self.cache.lock().put(blob.clone(), image.clone());
        Ok(blob)
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        let (prefix, rest) = hash.split_at(2.min(hash.len()));
        self.root.join(prefix).join(rest)
    }
}

fn encode_webp(image: &DynamicImage) -> Result<Vec<u8>> {
    let rgba = image.to_rgba8();
    let mut config = webp::WebPConfig::new()
        .map_err(|()| anyhow::anyhow!("initialize libwebp configuration"))?;
    config.lossless = 1;
    config.quality = 100.0;
    config.method = 6;
    config.alpha_compression = 1;
    config.alpha_quality = 100;
    config.pass = 10;
    config.thread_level = 1;
    config.exact = 1;

    let encoded = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
        .encode_advanced(&config)
        .map_err(|error| anyhow::anyhow!("encode lossless WebP: {error:?}"))?;
    Ok(encoded.to_vec())
}

fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::io::Cursor;
    use tempfile::tempdir;

    fn png_bytes(color: [u8; 4]) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, Rgba(color)));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn put_image_bytes_normalizes_to_lossless_webp() {
        let dir = tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let png = png_bytes([12, 34, 56, 78]);
        let blob = store.put_image_bytes(&png).unwrap();
        assert!(!blob.is_empty());
        let stored = store.get_bytes(&blob).unwrap();
        assert!(is_webp(&stored));
        assert_ne!(stored, png);
        assert_eq!(
            image::load_from_memory(&stored)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [12, 34, 56, 78]
        );
        assert!(store.exists(&blob));
    }

    #[test]
    fn same_pixels_produce_same_ref() {
        let dir = tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let png = png_bytes([1, 2, 3, 255]);
        let image = image::load_from_memory(&png).unwrap();
        let a = store.put_image_bytes(&png).unwrap();
        let b = store.put_webp(&image).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_non_image_bytes() {
        let dir = tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        assert!(store.put_image_bytes(b"not an image").is_err());
    }
}
