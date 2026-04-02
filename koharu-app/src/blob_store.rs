use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Write bytes to the store, return the blake3 hash hex string.
    pub fn put(&self, data: &[u8]) -> Result<String> {
        let hash = blake3::hash(data).to_hex().to_string();
        let path = self.blob_path(&hash);
        if path.exists() {
            return Ok(hash); // already stored (content-addressable)
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data).with_context(|| format!("Failed to write blob {hash}"))?;
        Ok(hash)
    }

    /// Read bytes from the store by hash.
    pub fn get(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(hash);
        std::fs::read(&path).with_context(|| format!("Blob not found: {hash}"))
    }

    /// Check if a blob exists.
    pub fn exists(&self, hash: &str) -> bool {
        self.blob_path(hash).exists()
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        let (prefix, rest) = hash.split_at(2.min(hash.len()));
        self.root.join(prefix).join(rest)
    }
}
