//! WGPU-backed interactive viewport for renderer compositions.
//!
//! The crate is split into three layers:
//!
//! - `geometry`, `transform`, and `mask` own transient interaction state;
//! - `gpu` owns the viewport-sized Vello target and asynchronous readbacks;
//! - `Canvas` combines those with an immutable `koharu_renderer::Composition`.
//!
//! `Canvas` is the facade connecting those layers. It owns no window or WGPU
//! surface: [`Canvas::render`] returns an offscreen texture for the desktop host
//! to present.

mod canvas;
mod damage;
mod error;
mod geometry;
mod gpu;
mod mask;
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

#[cfg(test)]
use geometry::frame_corners;
use gpu::GpuRenderer;
use mask::{ActiveStroke, MaskState};
use transform::ActiveTransform;
