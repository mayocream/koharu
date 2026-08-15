use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to encode prepared frame packet")]
    Encode(#[source] postcard::Error),
    #[error("failed to decode prepared frame packet")]
    Decode(#[source] postcard::Error),
    #[error("unsupported prepared frame manifest version {0}")]
    UnsupportedManifestVersion(u16),
    #[error("unsupported prepared resource version {0}")]
    UnsupportedResourceVersion(u16),
    #[error("invalid rasterizer input: {0}")]
    Invalid(String),
    #[error("rasterizer backend failed: {0}")]
    Backend(String),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub(crate) fn backend(error: impl std::fmt::Display) -> Self {
        Self::Backend(error.to_string())
    }
}
