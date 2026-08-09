#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid canvas state: {0}")]
    Invalid(String),
    #[error("canvas GPU error: {0}")]
    Gpu(String),
    #[error("no composition is installed")]
    NoComposition,
    #[error("no mask stroke is active")]
    NoStroke,
    #[error("no element transform is active")]
    NoTransform,
}

pub type Result<T> = std::result::Result<T, Error>;
