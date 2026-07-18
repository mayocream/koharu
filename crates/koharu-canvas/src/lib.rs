//! Rust-owned, WGPU-backed editor viewport for Koharu scenes.

mod canvas;
mod error;
mod geometry;
mod mask;
mod overlay;
mod resources;
mod state;

pub use canvas::{Canvas, CanvasFrame};
pub use error::{Error, Result};
pub use geometry::{Camera, PagePoint, PhysicalPoint, PhysicalSize, PixelRect, PixelSize};
pub use mask::MaskCommit;
pub use state::{
    BaseImage, Brush, BrushCursor, CanvasDiagnostic, CanvasGpu, CanvasOptions, Color, DisplayState,
    ElementPreview, Guide, Handle, HitTarget, MaskOverlay, MaskPlane, OverlayState, PageView,
    StrokeMode, ViewState,
};

use geometry::{frame_contains, frame_corners};
use mask::{ActiveStroke, MaskState};
use overlay::{OverlayGeometry, OverlayRenderer};
use resources::{ResourceEvent, ResourceKind, Resources};
