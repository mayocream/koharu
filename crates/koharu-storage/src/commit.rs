use std::{collections::BTreeSet, sync::Arc};

use revision::revisioned;

use crate::{Blob, BlobId, DocumentId, Revision};

/// One atomic document mutation supplied by the scene owner.
#[derive(Clone, Debug)]
pub struct Commit {
    pub(crate) document: DocumentId,
    pub(crate) parent: Revision,
    pub(crate) label: Option<Arc<str>>,
    pub(crate) forward: Vec<u8>,
    pub(crate) inverse: Vec<u8>,
    pub(crate) references: BTreeSet<BlobId>,
    pub(crate) blobs: Vec<Blob>,
}

impl Commit {
    #[must_use]
    pub fn new(document: DocumentId, parent: Revision, forward: Vec<u8>, inverse: Vec<u8>) -> Self {
        Self {
            document,
            parent,
            label: None,
            forward,
            inverse,
            references: BTreeSet::new(),
            blobs: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_label(mut self, label: Option<Arc<str>>) -> Self {
        self.label = label;
        self
    }

    pub fn reference_blobs(&mut self, ids: impl IntoIterator<Item = BlobId>) {
        self.references.extend(ids);
    }

    pub fn attach(&mut self, blob: Blob) {
        self.references.insert(blob.id());
        self.blobs.push(blob);
    }

    #[must_use]
    pub fn payload_len(&self) -> usize {
        self.forward.len().saturating_add(self.inverse.len())
    }
}

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub revision: Revision,
    pub label: Option<Arc<str>>,
    pub forward: Arc<[u8]>,
    pub inverse: Arc<[u8]>,
    pub blobs: Arc<[BlobId]>,
}

#[derive(Clone, Debug)]
pub struct Recovery {
    pub document: DocumentId,
    pub checkpoint_revision: Revision,
    pub head: Revision,
    pub checkpoint: Arc<[u8]>,
    pub entries: Vec<HistoryEntry>,
}

#[derive(Clone, Debug)]
pub struct Refresh {
    pub from: Revision,
    pub to: Revision,
    pub entries: Vec<HistoryEntry>,
    pub(crate) checkpoint_revision: Revision,
    pub(crate) commits_since_checkpoint: u64,
    pub(crate) bytes_since_checkpoint: u64,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug)]
pub(crate) struct StoredEntry {
    pub(crate) label: Option<String>,
    pub(crate) forward: Vec<u8>,
    pub(crate) inverse: Vec<u8>,
    pub(crate) blobs: Vec<BlobId>,
}

impl StoredEntry {
    pub(crate) fn into_history(self, revision: Revision) -> HistoryEntry {
        HistoryEntry {
            revision,
            label: self.label.map(Arc::from),
            forward: Arc::from(self.forward),
            inverse: Arc::from(self.inverse),
            blobs: Arc::from(self.blobs),
        }
    }
}
