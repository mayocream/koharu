use thiserror::Error;

#[derive(Debug, Error)]
pub enum PsdExportError {
    #[error("classic PSD only supports dimensions up to 30000x30000, got {width}x{height}")]
    UnsupportedDimensions { width: u32, height: u32 },
    #[error("renderer did not produce pixels for scene entity {0}")]
    MissingRenderedEntity(koharu_scene::EntityId),
    #[error("PSD layer count exceeds the classic format limit: {0}")]
    TooManyLayers(usize),
    #[error("invalid layer bounds for {layer}: {width}x{height}")]
    InvalidLayerBounds {
        layer: String,
        width: i32,
        height: i32,
    },
    #[error("RLE row {row} for {layer} exceeded PSD limits ({length} bytes)")]
    InvalidChannelEncoding {
        layer: String,
        row: usize,
        length: usize,
    },
    #[error("invalid descriptor data: {0}")]
    InvalidDescriptor(String),
    #[error(transparent)]
    Renderer(#[from] koharu_renderer::Error),
    #[error("PSD encoding task failed: {0}")]
    EncodingTask(tokio::task::JoinError),
}
