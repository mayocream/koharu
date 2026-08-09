//! WGPU-backed scene viewport and semantic edit preview for Koharu scenes.
//!
//! The crate is split into three layers:
//!
//! - `geometry`, `transform`, and most of `mask` validate UI input and carry
//!   most behavioral tests;
//! - `elements` and `resources` turn committed scene data into reusable Vello
//!   drawing descriptions;
//! - `gpu` contains the Vello target and the small amount of unavoidable WGPU
//!   texture management.
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
mod model;
mod resources;
mod state;
mod transform;
#[cfg(test)]
mod visual_tests;

pub use canvas::{Canvas, CanvasFrame};
pub use error::{Error, Result};
pub use geometry::{Camera, Frame, PagePoint, PhysicalPoint, PhysicalSize, PixelRect, PixelSize};
pub use mask::MaskCommit;
pub use state::{
    Brush, CanvasDiagnostic, CanvasGpu, CanvasOptions, Color, DisplayState, ElementFrame,
    ElementId, ElementPreview, MaskOverlay, MaskPlane, PageId, PageView, RasterStrokeCommit,
    StrokeMode, TransformCommit, ViewState,
};

use elements::{ElementSceneContext, ElementScenes};
#[cfg(test)]
use geometry::frame_corners;
use gpu::GpuRenderer;
use mask::{ActiveStroke, MaskState};
use model::{CanvasElement, CanvasPage};
use resources::{ResourceEvent, ResourceKind, Resources};
use transform::ActiveTransform;
