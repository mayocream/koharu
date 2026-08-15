//! Bounded decoded-image storage and validation.

use std::{collections::HashMap, sync::Arc};

use koharu_scene::BlobId;

use crate::{Error, Result};

pub(crate) const DEFAULT_IMAGE_CACHE_BYTES: usize = 512 * 1024 * 1024;

pub(crate) struct DecodedImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) encoded: Arc<[u8]>,
    pub(crate) pixels: Arc<[u8]>,
}

impl DecodedImage {
    fn byte_len(&self) -> usize {
        self.encoded.len().saturating_add(self.pixels.len())
    }
}

struct CachedImage {
    image: Arc<DecodedImage>,
    last_used: u64,
}

pub(crate) struct ImageCache {
    entries: HashMap<BlobId, CachedImage>,
    max_bytes: usize,
    bytes: usize,
    clock: u64,
}

impl ImageCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            max_bytes: DEFAULT_IMAGE_CACHE_BYTES,
            bytes: 0,
            clock: 0,
        }
    }

    #[cfg(test)]
    fn with_limit(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            ..Self::new()
        }
    }

    pub(crate) fn get(&mut self, blob: BlobId) -> Option<Arc<DecodedImage>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(&blob)?;
        entry.last_used = self.clock;
        Some(entry.image.clone())
    }

    pub(crate) fn insert(&mut self, blob: BlobId, image: Arc<DecodedImage>) {
        self.clock = self.clock.wrapping_add(1);
        if let Some(previous) = self.entries.remove(&blob) {
            self.bytes = self.bytes.saturating_sub(previous.image.byte_len());
        }
        if image.byte_len() > self.max_bytes {
            return;
        }
        self.bytes = self.bytes.saturating_add(image.byte_len());
        self.entries.insert(
            blob,
            CachedImage {
                image,
                last_used: self.clock,
            },
        );
        while self.bytes > self.max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(blob, _)| *blob)
            else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(removed.image.byte_len());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_larger_than_the_budget_is_not_cached() {
        let blob = BlobId::for_bytes(b"oversized");
        let image = Arc::new(DecodedImage {
            width: 2,
            height: 1,
            encoded: Arc::from([0_u8; 1]),
            pixels: Arc::from([0_u8; 8]),
        });
        let mut cache = ImageCache::with_limit(4);
        cache.insert(blob, image);
        assert!(cache.get(blob).is_none());
        assert_eq!(cache.bytes, 0);
    }
}

pub(crate) fn decode(
    blob: BlobId,
    bytes: Arc<[u8]>,
    expected: Option<(u32, u32)>,
) -> Result<(BlobId, Arc<DecodedImage>)> {
    let decoded = image::load_from_memory(&bytes)
        .map_err(|source| Error::Image { blob, source })?
        .into_rgba8();
    if expected.is_some_and(|size| size != decoded.dimensions()) {
        return Err(Error::invalid(format!(
            "blob {blob} decoded as {}x{}, expected {:?}",
            decoded.width(),
            decoded.height(),
            expected
        )));
    }
    Ok((
        blob,
        Arc::new(DecodedImage {
            width: decoded.width(),
            height: decoded.height(),
            encoded: bytes,
            pixels: Arc::from(decoded.into_raw()),
        }),
    ))
}
