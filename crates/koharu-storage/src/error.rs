use crate::{BlobId, DocumentId, Revision};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("storage database error: {0}")]
    Database(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("failed to persist storage changes: {0}")]
    Durability(String),
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

macro_rules! database_error {
    ($($error:ty),+ $(,)?) => {
        $(
            impl From<$error> for Error {
                fn from(error: $error) -> Self {
                    Self::Database(Box::new(error))
                }
            }
        )+
    };
}

database_error!(
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError,
    redb::CompactionError,
    redb::SetDurabilityError,
);

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
