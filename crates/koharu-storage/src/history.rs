use std::{collections::BTreeSet, sync::Arc};

use revision::revisioned;

use crate::{BlobAttachment, BlobId, DocumentId, Revision};

/// One opaque document commit read from durable storage.
#[derive(Clone, Debug)]
pub struct Commit {
    pub revision: Revision,
    pub label: Option<Arc<str>>,
    pub forward: Arc<[u8]>,
    pub inverse: Arc<[u8]>,
    pub blobs: Arc<[BlobId]>,
}

/// A complete recovery stream: one checkpoint followed by its commit tail.
#[derive(Clone, Debug)]
pub struct Recovery {
    pub document: DocumentId,
    pub checkpoint_revision: Revision,
    pub head: Revision,
    pub checkpoint: Arc<[u8]>,
    pub commits: Vec<Commit>,
}

/// A validated refresh candidate. The session advances only after its owner
/// has successfully applied every opaque commit.
#[derive(Clone, Debug)]
pub struct Refresh {
    pub from: Revision,
    pub to: Revision,
    pub commits: Vec<Commit>,
}

#[derive(Clone, Debug)]
pub struct CommitRequest {
    pub document: DocumentId,
    pub parent: Revision,
    pub label: Option<Arc<str>>,
    pub forward: Vec<u8>,
    pub inverse: Vec<u8>,
    pub blobs: BTreeSet<BlobId>,
    pub attachments: Vec<BlobAttachment>,
}

impl CommitRequest {
    #[must_use]
    pub fn new(document: DocumentId, parent: Revision, forward: Vec<u8>, inverse: Vec<u8>) -> Self {
        Self {
            document,
            parent,
            label: None,
            forward,
            inverse,
            blobs: BTreeSet::new(),
            attachments: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_label(mut self, label: Option<Arc<str>>) -> Self {
        self.label = label;
        self
    }

    pub fn reference_blobs(&mut self, blobs: impl IntoIterator<Item = BlobId>) {
        self.blobs.extend(blobs);
    }

    pub fn attach(&mut self, attachment: BlobAttachment) {
        self.blobs.insert(attachment.id());
        self.attachments.push(attachment);
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug)]
pub(crate) struct StoredCommit {
    pub(crate) label: Option<String>,
    pub(crate) forward: Vec<u8>,
    pub(crate) inverse: Vec<u8>,
    pub(crate) blobs: Vec<BlobId>,
}

impl StoredCommit {
    pub(crate) fn into_commit(self, revision: Revision) -> Commit {
        Commit {
            revision,
            label: self.label.map(Arc::from),
            forward: Arc::from(self.forward),
            inverse: Arc::from(self.inverse),
            blobs: Arc::from(self.blobs),
        }
    }
}
