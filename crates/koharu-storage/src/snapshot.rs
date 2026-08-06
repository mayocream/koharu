use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{Blob, BlobBatch, BlobId, DocumentId, Error, Result, Revision, database::Engine};

/// Immutable access to the blobs referenced by one document revision.
#[derive(Clone)]
pub struct Snapshot {
    document: DocumentId,
    revision: Revision,
    engine: Arc<Engine>,
    preview: Arc<BTreeMap<BlobId, Arc<[u8]>>>,
    lease: Arc<BTreeSet<BlobId>>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Snapshot")
            .field("document", &self.document)
            .field("revision", &self.revision)
            .field("blobs", &self.lease.len())
            .field("preview_blobs", &self.preview.len())
            .finish()
    }
}

impl Snapshot {
    pub(crate) fn durable(
        document: DocumentId,
        revision: Revision,
        engine: Arc<Engine>,
        referenced: BTreeSet<BlobId>,
    ) -> Result<Self> {
        if let Some(id) = engine.missing_blobs(&referenced)?.first() {
            return Err(Error::BlobNotFound(*id));
        }
        let lease = engine.lease(referenced);
        Ok(Self {
            document,
            revision,
            engine,
            preview: Arc::new(BTreeMap::new()),
            lease,
        })
    }

    pub fn preview(
        &self,
        revision: Revision,
        blobs: impl IntoIterator<Item = Blob>,
        referenced: BTreeSet<BlobId>,
    ) -> Result<Self> {
        let mut preview = (*self.preview).clone();
        preview.retain(|id, _| referenced.contains(id));
        for blob in blobs {
            if referenced.contains(&blob.id()) {
                preview.insert(blob.id(), blob.bytes());
            }
        }
        let durable = referenced
            .iter()
            .filter(|id| !preview.contains_key(id))
            .copied()
            .collect();
        if let Some(id) = self.engine.missing_blobs(&durable)?.first() {
            return Err(Error::BlobNotFound(*id));
        }
        let lease = self.engine.lease(referenced);
        Ok(Self {
            document: self.document,
            revision,
            engine: self.engine.clone(),
            preview: Arc::new(preview),
            lease,
        })
    }

    #[must_use]
    pub const fn document_id(&self) -> DocumentId {
        self.document
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub fn has_blob(&self, id: BlobId) -> bool {
        self.preview.contains_key(&id) || self.lease.contains(&id)
    }

    pub fn read_blob(&self, id: BlobId) -> Result<Arc<[u8]>> {
        if let Some(bytes) = self.preview.get(&id) {
            return Ok(bytes.clone());
        }
        if !self.lease.contains(&id) {
            return Err(Error::BlobNotFound(id));
        }
        self.engine.read_blob(id)
    }

    pub fn read_blobs(&self, ids: impl IntoIterator<Item = BlobId>) -> Result<BlobBatch> {
        let ids = ids.into_iter().collect::<BTreeSet<_>>();
        if let Some(id) = ids.iter().find(|id| !self.has_blob(**id)) {
            return Err(Error::BlobNotFound(*id));
        }
        let durable = ids
            .iter()
            .filter(|id| !self.preview.contains_key(id))
            .copied()
            .collect();
        let mut batch = self.engine.read_blobs(&durable)?;
        for id in ids {
            if let Some(bytes) = self.preview.get(&id) {
                batch.blobs.insert(id, bytes.clone());
            }
        }
        Ok(batch)
    }
}
