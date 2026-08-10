#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid canvas state: {0}")]
    Invalid(String),
    #[error("canvas GPU error: {0}")]
    Gpu(String),
    #[error("no renderer frame is active")]
    NoFrame,
    #[error("no mask or raster stroke is active")]
    NoStroke,
    #[error("no element transform is active")]
    NoTransform,
}

pub type Result<T> = std::result::Result<T, Error>;
