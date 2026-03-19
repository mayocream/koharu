use thiserror::Error;

#[derive(Debug, Error)]
pub enum PsdExportError {
    #[error("classic PSD only supports dimensions up to 30000x30000, got {width}x{height}")]
    UnsupportedDimensions { width: u32, height: u32 },
    #[error("document is missing base image data")]
    MissingBaseImage,
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
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
