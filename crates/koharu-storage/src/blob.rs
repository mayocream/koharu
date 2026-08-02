use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use redb::ReadableDatabase;

use crate::{
    BlobId, Error, Result,
    storage::{BLOBS, Store},
};

#[derive(Clone, Debug)]
pub struct BlobAttachment {
    id: BlobId,
    bytes: Arc<[u8]>,
}

impl BlobAttachment {
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
    blobs: BTreeMap<BlobId, Arc<[u8]>>,
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

    pub(crate) fn insert(&mut self, id: BlobId, bytes: Arc<[u8]>) {
        self.blobs.insert(id, bytes);
    }
}

impl Store {
    pub(crate) fn contains_blob(&self, id: BlobId) -> Result<bool> {
        if self.cache.lock().contains(id) {
            return Ok(true);
        }
        let transaction = self.database.read().begin_read()?;
        let table = transaction.open_table(BLOBS)?;
        Ok(table.get(id)?.is_some())
    }

    pub(crate) fn read_blob(&self, id: BlobId) -> Result<Arc<[u8]>> {
        if let Some(bytes) = self.cache.lock().get(id) {
            return Ok(bytes);
        }
        let transaction = self.database.read().begin_read()?;
        let table = transaction.open_table(BLOBS)?;
        let stored = table.get(id)?.ok_or(Error::BlobNotFound(id))?;
        let bytes = stored.value();
        if BlobId::for_bytes(bytes) != id {
            return Err(Error::invalid(format!("blob {id} has invalid content")));
        }
        let bytes: Arc<[u8]> = Arc::from(bytes);
        self.cache.lock().insert(id, bytes.clone());
        Ok(bytes)
    }

    pub(crate) fn read_blobs(&self, ids: impl IntoIterator<Item = BlobId>) -> Result<BlobBatch> {
        let ids = ids.into_iter().collect::<BTreeSet<_>>();
        let mut batch = BlobBatch::default();
        let mut missing = Vec::new();
        {
            let mut cache = self.cache.lock();
            for id in &ids {
                if let Some(bytes) = cache.get(*id) {
                    batch.blobs.insert(*id, bytes);
                } else {
                    missing.push(*id);
                }
            }
        }
        if missing.is_empty() {
            return Ok(batch);
        }

        let transaction = self.database.read().begin_read()?;
        let table = transaction.open_table(BLOBS)?;
        let mut loaded = Vec::with_capacity(missing.len());
        for id in missing {
            let stored = table.get(id)?.ok_or(Error::BlobNotFound(id))?;
            if BlobId::for_bytes(stored.value()) != id {
                return Err(Error::invalid(format!("blob {id} has invalid content")));
            }
            let bytes: Arc<[u8]> = Arc::from(stored.value());
            batch.blobs.insert(id, bytes.clone());
            loaded.push((id, bytes));
        }
        let mut cache = self.cache.lock();
        for (id, bytes) in loaded {
            cache.insert(id, bytes);
        }
        Ok(batch)
    }

    pub(crate) fn lease(&self, blobs: BTreeSet<BlobId>) -> Arc<BTreeSet<BlobId>> {
        let lease = Arc::new(blobs);
        let mut leases = self.leases.lock();
        leases.retain(|lease| lease.strong_count() != 0);
        leases.push(Arc::downgrade(&lease));
        lease
    }

    pub(crate) fn live_blobs(&self) -> BTreeSet<BlobId> {
        let mut result = BTreeSet::new();
        let mut leases = self.leases.lock();
        leases.retain(|lease| {
            if let Some(lease) = lease.upgrade() {
                result.extend(lease.iter().copied());
                true
            } else {
                false
            }
        });
        result
    }

    pub(crate) fn invalidate_blobs(&self, ids: &BTreeSet<BlobId>) {
        let mut cache = self.cache.lock();
        for id in ids {
            cache.remove(*id);
        }
    }
}
