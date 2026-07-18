//! Wry desktop shell and WGPU presenter for Koharu's Rust-owned canvas.

mod app;
mod gpu;
mod mask;
mod protocol;

pub use app::{
    Application, CustomProtocol, DesktopContext, DesktopHandle, Frontend, Options, ProtocolRequest,
    ProtocolResponder, ProtocolResponse, run,
};
pub use gpu::{Gpu, PhysicalRect};
pub use mask::{EncodedMask, MaskEncodingError, MaskEncodingResult};
