//! WGPU-backed interactive viewport over an immutable renderer frame.
//!
//! The renderer owns document traversal, decoding, text layout, and retained
//! visual nodes. Canvas owns only viewport state, damage, transient edits,
//! sparse masks, and nonblocking GPU readback.

mod canvas;
mod damage;
mod error;
mod geometry;
mod gpu;
mod mask;
mod raster;
mod state;
mod transform;

pub use canvas::{Canvas, CanvasFrame};
pub use error::{Error, Result};
pub use geometry::{Camera, Frame, PagePoint, PhysicalPoint, PhysicalSize, PixelRect, PixelSize};
pub use mask::MaskCommit;
pub use state::{
    Brush, CanvasGpu, CanvasOptions, Color, ElementFrame, ElementId, ElementPreview, MaskOverlay,
    MaskTarget, PageId, RasterStrokeCommit, StrokeMode, TransformCommit, ViewState,
};

use gpu::GpuRenderer;
use mask::{ActiveStroke, MaskState};
use transform::{ActiveTransform, TransformState};
