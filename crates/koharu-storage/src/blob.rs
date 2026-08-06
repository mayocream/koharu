use std::{collections::BTreeMap, sync::Arc};

use lru::LruCache;

use crate::BlobId;

/// Content-addressed bytes entering storage.
#[derive(Clone, Debug)]
pub struct Blob {
    id: BlobId,
    bytes: Arc<[u8]>,
}

impl Blob {
    #[must_use]
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        let bytes = bytes.into();
        let id = BlobId::for_bytes(&bytes);
        Self { id, bytes }
    }

    #[must_use]
    pub const fn id(&self) -> BlobId {
        self.id
    }

    #[must_use]
    pub fn bytes(&self) -> Arc<[u8]> {
        self.bytes.clone()
    }
}

#[derive(Clone, Debug, Default)]
pub struct BlobBatch {
    pub(crate) blobs: BTreeMap<BlobId, Arc<[u8]>>,
}

impl BlobBatch {
    #[must_use]
    pub fn get(&self, id: BlobId) -> Option<&Arc<[u8]>> {
        self.blobs.get(&id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (BlobId, &Arc<[u8]>)> {
        self.blobs.iter().map(|(id, bytes)| (*id, bytes))
    }
}

pub(crate) struct BlobCache {
    entries: LruCache<BlobId, Arc<[u8]>>,
    bytes: usize,
    limit: usize,
}

impl BlobCache {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            entries: LruCache::unbounded(),
            bytes: 0,
            limit,
        }
    }

    pub(crate) fn get(&mut self, id: BlobId) -> Option<Arc<[u8]>> {
        self.entries.get(&id).cloned()
    }

    pub(crate) fn insert(&mut self, id: BlobId, bytes: Arc<[u8]>) {
        if self.limit == 0 || bytes.len() > self.limit {
            return;
        }
        if let Some(previous) = self.entries.put(id, bytes.clone()) {
            self.bytes = self.bytes.saturating_sub(previous.len());
        }
        self.bytes = self.bytes.saturating_add(bytes.len());
        while self.bytes > self.limit {
            let Some((_, removed)) = self.entries.pop_lru() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(removed.len());
        }
    }

    pub(crate) fn remove(&mut self, id: BlobId) {
        if let Some(bytes) = self.entries.pop(&id) {
            self.bytes = self.bytes.saturating_sub(bytes.len());
        }
    }
}
