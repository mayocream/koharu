use crate::{ElementId, PageId, Revision};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Codec(#[from] revision::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error("not a Koharu project")]
    NotAProject,
    #[error("unsupported Koharu project schema {0}")]
    UnsupportedSchema(u32),
    #[error("page {0} was not found")]
    PageNotFound(PageId),
    #[error("element {0} was not found")]
    ElementNotFound(ElementId),
    #[error("element {0} is not the requested kind")]
    ElementKind(ElementId),
    #[error("revision conflict: expected {expected}, current revision is {actual}")]
    RevisionConflict {
        expected: Revision,
        actual: Revision,
    },
    #[error("command batches conflict")]
    CommandConflict,
    #[error("revision {0} is no longer retained")]
    HistoryNotFound(Revision),
    #[error("history no longer matches the current project: {0}")]
    HistoryConflict(String),
    #[error("invalid scene data: {0}")]
    Invalid(String),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
