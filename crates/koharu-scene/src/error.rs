use crate::{NodeId, PageId, Revision};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SQLite error: {0}")]
    Sqlite(rusqlite::Error),

    #[error("scene encoding error: {0}")]
    Encode(#[from] postcard::Error),

    #[error("image metadata error: {0}")]
    Image(#[from] image::ImageError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("page {0} does not exist")]
    PageNotFound(PageId),

    #[error("node {0} does not exist")]
    NodeNotFound(NodeId),

    #[error("page {0} already exists")]
    PageAlreadyExists(PageId),

    #[error("node {0} already exists")]
    NodeAlreadyExists(NodeId),

    #[error("node {node} is {actual}, expected {expected}")]
    WrongNodeKind {
        node: NodeId,
        expected: &'static str,
        actual: &'static str,
    },

    #[error("invalid scene command: {0}")]
    InvalidCommand(String),

    #[error("session is at revision {local}, but SQLite is at {durable}; refresh it first")]
    StaleSession { local: Revision, durable: Revision },

    #[error("command is based on revision {base}, but the current revision is {current}")]
    RevisionConflict { base: Revision, current: Revision },

    #[error("command ID was already used for different content")]
    CommandIdConflict,

    #[error("history precondition failed: {0}")]
    HistoryConflict(String),

    #[error("revision {0} is not retained")]
    RevisionNotRetained(Revision),

    #[error("session is poisoned after an internal apply failure; reopen it")]
    Poisoned,

    #[error("database is busy")]
    Busy,

    #[error("unsupported schema version {0}")]
    UnsupportedSchema(u32),

    #[error("database is not a koharu-scene project")]
    NotAProject,
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidCommand(message.into())
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        if matches!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
        ) {
            Self::Busy
        } else {
            Self::Sqlite(error)
        }
    }
}
