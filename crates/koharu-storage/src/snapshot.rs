use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{
    BlobBatch, BlobId, ComponentAddress, ComponentKey, ComponentRecord, DocumentId, Edit, Patch,
    RecordId, RecordRef, Result, Revision, blob::BlobStore, state::State,
};

#[derive(Clone)]
pub struct Snapshot {
    pub(crate) state: Arc<State>,
    pub(crate) blobs: Arc<BlobStore>,
    overlay: Arc<BTreeMap<BlobId, Arc<[u8]>>>,
    _lease: Arc<BTreeSet<BlobId>>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Snapshot")
            .field("document", &self.state.document)
            .field("revision", &self.state.revision)
            .field("records", &self.state.records.len())
            .finish()
    }
}

impl Snapshot {
    pub(crate) fn new(state: Arc<State>, blobs: Arc<BlobStore>) -> Self {
        let lease = blobs.lease(state.referenced_blobs());
        Self {
            state,
            blobs,
            overlay: Arc::new(BTreeMap::new()),
            _lease: lease,
        }
    }

    #[must_use]
    pub fn document_id(&self) -> DocumentId {
        self.state.document
    }

    #[must_use]
    pub fn revision(&self) -> Revision {
        self.state.revision
    }

    #[must_use]
    pub fn root(&self) -> RecordId {
        self.state.root
    }

    pub fn record(&self, id: RecordId) -> Result<RecordRef<'_>> {
        self.state.record(id)
    }

    #[must_use]
    pub fn contains_record(&self, id: RecordId) -> bool {
        self.state.records.contains_key(&id)
    }

    pub fn records(&self) -> impl Iterator<Item = RecordRef<'_>> {
        self.state.records()
    }

    pub fn component(
        &self,
        record: RecordId,
        key: &ComponentKey,
    ) -> Result<Option<&ComponentRecord>> {
        self.state.component(record, key)
    }

    pub fn incoming_references(
        &self,
        record: RecordId,
    ) -> Result<impl Iterator<Item = &ComponentAddress>> {
        self.state.incoming(record)
    }

    #[must_use]
    pub fn has_blob(&self, id: BlobId) -> bool {
        self.overlay.contains_key(&id) || self.blobs.contains(id).unwrap_or(false)
    }

    pub fn read_blob(&self, id: BlobId) -> Result<Arc<[u8]>> {
        self.overlay
            .get(&id)
            .cloned()
            .map_or_else(|| self.blobs.read(id), Ok)
    }

    pub fn read_blobs(&self, ids: impl IntoIterator<Item = BlobId>) -> Result<BlobBatch> {
        let ids = ids.into_iter().collect::<BTreeSet<_>>();
        let mut batch = self.blobs.read_many(
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

    #[must_use]
    pub fn edit(&self) -> Edit {
        Edit::new(self.state.clone())
    }

    pub fn patch(&self, f: impl FnOnce(&mut Edit) -> Result<()>) -> Result<Patch> {
        let mut edit = self.edit();
        f(&mut edit)?;
        edit.finish()
    }

    pub fn preview<'a>(&self, patches: impl IntoIterator<Item = &'a Patch>) -> Result<Self> {
        let mut state = (*self.state).clone();
        let mut overlay = (*self.overlay).clone();
        let mut affected_blobs = BTreeSet::new();
        for patch in patches {
            for operation in patch.operations() {
                operation.blob_refs(&mut affected_blobs);
            }
            let (next, attachments) = patch.apply(&state)?;
            state = next;
            overlay.extend(attachments);
        }
        for id in affected_blobs {
            if state.references_blob(id)
                && !overlay.contains_key(&id)
                && !self.blobs.contains(id)?
            {
                return Err(crate::Error::BlobNotFound(id));
            }
        }
        let state = Arc::new(state);
        let lease = self.blobs.lease(state.referenced_blobs());
        Ok(Self {
            state,
            blobs: self.blobs.clone(),
            overlay: Arc::new(overlay),
            _lease: lease,
        })
    }
}
