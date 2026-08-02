use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{BlobAttachment, BlobBatch, BlobId, DocumentId, Result, Revision, storage::Store};

/// A revision-pinned view of document blobs. Document state itself belongs to
/// the crate that interprets the opaque checkpoint and commit payloads.
#[derive(Clone)]
pub struct Snapshot {
    document: DocumentId,
    revision: Revision,
    store: Arc<Store>,
    overlay: Arc<BTreeMap<BlobId, Arc<[u8]>>>,
    _lease: Arc<BTreeSet<BlobId>>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Snapshot")
            .field("document", &self.document)
            .field("revision", &self.revision)
            .field("overlay_blobs", &self.overlay.len())
            .finish()
    }
}

impl Snapshot {
    pub(crate) fn new(
        document: DocumentId,
        revision: Revision,
        store: Arc<Store>,
        referenced: BTreeSet<BlobId>,
    ) -> Self {
        let lease = store.lease(referenced);
        Self {
            document,
            revision,
            store,
            overlay: Arc::new(BTreeMap::new()),
            _lease: lease,
        }
    }

    pub fn preview(
        &self,
        revision: Revision,
        attachments: impl IntoIterator<Item = BlobAttachment>,
        referenced: BTreeSet<BlobId>,
    ) -> Result<Self> {
        let mut overlay = (*self.overlay).clone();
        for attachment in attachments {
            overlay.insert(attachment.id(), attachment.bytes());
        }
        for id in &referenced {
            if !overlay.contains_key(id) && !self.store.contains_blob(*id)? {
                return Err(crate::Error::BlobNotFound(*id));
            }
        }
        let lease = self.store.lease(referenced);
        Ok(Self {
            document: self.document,
            revision,
            store: self.store.clone(),
            overlay: Arc::new(overlay),
            _lease: lease,
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
        self.overlay.contains_key(&id) || self.store.contains_blob(id).unwrap_or(false)
    }

    pub fn read_blob(&self, id: BlobId) -> Result<Arc<[u8]>> {
        self.overlay
            .get(&id)
            .cloned()
            .map_or_else(|| self.store.read_blob(id), Ok)
    }

    pub fn read_blobs(&self, ids: impl IntoIterator<Item = BlobId>) -> Result<BlobBatch> {
        let ids = ids.into_iter().collect::<BTreeSet<_>>();
        let mut batch = self.store.read_blobs(
            ids.iter()
                .filter(|id| !self.overlay.contains_key(id))
                .copied(),
        )?;
        for id in ids {
            if let Some(bytes) = self.overlay.get(&id) {
                batch.insert(id, bytes.clone());
            }
        }
        Ok(batch)
    }
}
