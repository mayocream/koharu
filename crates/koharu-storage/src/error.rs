use crate::{BlobId, DocumentId, Revision};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    RocksDb(#[from] rocksdb::Error),
    #[error(transparent)]
    Codec(#[from] revision::Error),
    #[error("not a Koharu project database")]
    NotAProject,
    #[error("unsupported Koharu project schema {0}")]
    UnsupportedSchema(u32),
    #[error("patch belongs to document {patch}, not {session}")]
    DocumentMismatch {
        patch: DocumentId,
        session: DocumentId,
    },
    #[error("blob {0} was not found")]
    BlobNotFound(BlobId),
    #[error("revision conflict: expected {expected}, current revision is {actual}")]
    RevisionConflict {
        expected: Revision,
        actual: Revision,
    },
    #[error("revision {0} is no longer retained")]
    HistoryNotFound(Revision),
    #[error("invalid storage data: {0}")]
    Invalid(String),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
