use koharu_scene::{PageId, Revision};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Scene(#[from] koharu_scene::Error),
    #[error("invalid canvas state: {0}")]
    Invalid(String),
    #[error("canvas GPU error: {0}")]
    Gpu(String),
    #[error("page {page} changed from revision {expected} to {actual}")]
    RevisionConflict {
        page: PageId,
        expected: Revision,
        actual: Revision,
    },
    #[error("page {page} has uncommitted {plane} mask edits")]
    MaskConflict { page: PageId, plane: &'static str },
    #[error("no page is active")]
    NoPage,
    #[error("no mask stroke is active")]
    NoStroke,
}

pub type Result<T> = std::result::Result<T, Error>;
