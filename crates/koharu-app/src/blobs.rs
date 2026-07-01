//! Content-addressed blob storage for a `ProjectSession`.
//!
//! Blobs live at `.khrproj/blobs/ab/cdef…` (hex blake3 hash, sharded by the
//! first two chars). Immutable: a blob with a given hash is always the same
//! bytes. An in-memory LRU decodes images on demand.
//!
//! `BlobRef` itself lives in `koharu-core::blob`; this module only provides
//! the filesystem store + cache.

use std::io::Cursor;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::{DynamicImage, RgbaImage};
use koharu_core::BlobRef;
use lru::LruCache;
use parking_lot::Mutex;

const IMAGE_CACHE_CAPACITY: usize = 64;
const RAW_MAGIC: &[u8; 4] = b"RGBA";

/// Content-addressed blob store + decoded-image LRU.
pub struct BlobStore {
    root: PathBuf,
    cache: Mutex<LruCache<BlobRef, DynamicImage>>,
}

impl BlobStore {
    /// Open (or create) the store at `root`. Directory is created if missing.
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

    /// Root directory on disk.
    pub fn root(&self) -> &Path {
        &self.root
    }

    // --- raw bytes ---------------------------------------------------------

    /// Write raw bytes; return the blake3-derived `BlobRef`.
    pub fn put_bytes(&self, data: &[u8]) -> Result<BlobRef> {
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

    /// Read raw bytes by `BlobRef`.
    pub fn get_bytes(&self, r: &BlobRef) -> Result<Vec<u8>> {
        let path = self.blob_path(r.hash());
        std::fs::read(&path).with_context(|| format!("blob not found: {}", r.hash()))
    }

    /// Whether a blob exists on disk (no decode, no cache touch).
    pub fn exists(&self, r: &BlobRef) -> bool {
        self.blob_path(r.hash()).exists()
    }

    // --- decoded images ----------------------------------------------------

    /// Load and decode an image, using the LRU. Returns a cheap clone.
    pub fn load_image(&self, r: &BlobRef) -> Result<DynamicImage> {
        if let Some(img) = self.cache.lock().get(r).cloned() {
            return Ok(img);
        }
        let bytes = self.get_bytes(r)?;
        let img = decode_blob(&bytes)?;
        self.cache.lock().put(r.clone(), img.clone());
        Ok(img)
    }

    /// Encode an image as WebP, store, cache, return ref.
    pub fn put_webp(&self, img: &DynamicImage) -> Result<BlobRef> {
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::WebP)?;
        let r = self.put_bytes(&buf.into_inner())?;
        self.cache.lock().put(r.clone(), img.clone());
        Ok(r)
    }

    /// Store an image as raw RGBA with a 12-byte header. Cheap encode, used
    /// for sprites where WebP's compression gain doesn't justify its cost.
    pub fn put_raw(&self, img: &DynamicImage) -> Result<BlobRef> {
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let pixels = rgba.as_raw();
        let mut buf = Vec::with_capacity(12 + pixels.len());
        buf.extend_from_slice(RAW_MAGIC);
        buf.extend_from_slice(&w.to_le_bytes());
        buf.extend_from_slice(&h.to_le_bytes());
        buf.extend_from_slice(pixels);
        let r = self.put_bytes(&buf)?;
        self.cache.lock().put(r.clone(), img.clone());
        Ok(r)
    }

    /// Whether a blob uses our raw-RGBA wrapper (vs a standard image format).
    pub fn is_raw_rgba(&self, r: &BlobRef) -> bool {
        self.get_bytes(r)
            .map(|bytes| bytes.len() >= 4 && &bytes[..4] == RAW_MAGIC)
            .unwrap_or(false)
    }

    // --- internals ---------------------------------------------------------

    fn blob_path(&self, hash: &str) -> PathBuf {
        let (prefix, rest) = hash.split_at(2.min(hash.len()));
        self.root.join(prefix).join(rest)
    }
}

fn decode_blob(bytes: &[u8]) -> Result<DynamicImage> {
    if bytes.len() >= 12 && &bytes[..4] == RAW_MAGIC {
        let w = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let h = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let pixels = bytes[12..].to_vec();
        let img = RgbaImage::from_raw(w, h, pixels).context("invalid raw RGBA blob dimensions")?;
        return Ok(DynamicImage::ImageRgba8(img));
    }
    Ok(image::load_from_memory(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn put_and_get_round_trip() {
        let dir = tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let r = store.put_bytes(b"hello world").unwrap();
        assert!(!r.is_empty());
        let bytes = store.get_bytes(&r).unwrap();
        assert_eq!(bytes, b"hello world");
        assert!(store.exists(&r));
    }

    #[test]
    fn same_bytes_same_ref() {
        let dir = tempdir().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let a = store.put_bytes(b"x").unwrap();
        let b = store.put_bytes(b"x").unwrap();
        assert_eq!(a, b);
    }
}
