//! Rust-owned, WGPU-backed editor viewport for Koharu scenes.
//!
//! The crate is split into three layers:
//!
//! - `geometry`, `transform`, and most of `mask` are pure Rust interaction
//!   logic and should carry most behavioral tests;
//! - `elements` and `resources` turn committed scene data into reusable Vello
//!   drawing descriptions;
//! - `gpu` and `overlay` contain the WGPU-specific texture and shader code.
//!
//! `Canvas` is the facade connecting those layers. It owns no window or WGPU
//! surface: [`Canvas::render`] returns an offscreen texture for the desktop host
//! to present.

mod canvas;
mod damage;
mod elements;
mod error;
mod geometry;
mod gpu;
mod mask;
mod overlay;
mod resources;
mod state;
mod transform;
#[cfg(test)]
mod visual_tests;

pub use canvas::{Canvas, CanvasFrame};
pub use error::{Error, Result};
pub use geometry::{Camera, PagePoint, PhysicalPoint, PhysicalSize, PixelRect, PixelSize};
pub use mask::MaskCommit;
pub use state::{
    BaseImage, Brush, BrushCursor, CanvasDiagnostic, CanvasGpu, CanvasOptions, Color, DisplayState,
    ElementPreview, Guide, Handle, HitTarget, MaskOverlay, MaskPlane, OverlayState, PageView,
    StrokeMode, TransformCommit, ViewState,
};

use elements::{ElementSceneContext, ElementScenes};
use geometry::{frame_contains, frame_corners};
use gpu::GpuRenderer;
use mask::{ActiveStroke, MaskState};
use overlay::{OverlayGeometry, OverlayRenderer};
use resources::{ResourceEvent, ResourceKind, Resources};
use transform::ActiveTransform;
