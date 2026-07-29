use crate::{BlobId, DocumentId, RecordId, Revision};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Codec(#[from] revision::Error),
    #[error("not a Koharu storage document")]
    NotADocument,
    #[error("unsupported Koharu storage schema {0}")]
    UnsupportedSchema(u32),
    #[error("patch belongs to document {patch}, not {session}")]
    DocumentMismatch {
        patch: DocumentId,
        session: DocumentId,
    },
    #[error("record {0} was not found")]
    RecordNotFound(RecordId),
    #[error("record {0} already exists")]
    RecordAlreadyExists(RecordId),
    #[error("record {record} is referenced by {count} component(s)")]
    RecordReferenced { record: RecordId, count: usize },
    #[error("the permanent root record cannot be removed")]
    RootRemoval,
    #[error("blob {0} was not found")]
    BlobNotFound(BlobId),
    #[error("revision conflict: expected {expected}, current revision is {actual}")]
    RevisionConflict {
        expected: Revision,
        actual: Revision,
    },
    #[error("patch conflict: {0}")]
    PatchConflict(String),
    #[error("revision {0} is no longer retained")]
    HistoryNotFound(Revision),
    #[error("history no longer matches the current document: {0}")]
    HistoryConflict(String),
    #[error("invalid storage data: {0}")]
    Invalid(String),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub(crate) fn patch_conflict(message: impl Into<String>) -> Self {
        Self::PatchConflict(message.into())
    }
}
