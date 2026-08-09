use std::sync::Arc;

use koharu_scene::{EntityId, Geometry};
use vello::wgpu;

use crate::{Camera, Frame, PagePoint, PhysicalSize};

pub type PageId = EntityId;
pub type ElementId = EntityId;

pub type Color = [u8; 4];

/// Host-created GPU objects shared with the desktop presenter.
/// The canvas deliberately does not create its own adapter or device.
#[derive(Clone)]
pub struct CanvasGpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

/// Presentation policy owned by one interactive viewport.
#[derive(Clone, Debug)]
pub struct CanvasOptions {
    pub workspace_color: Color,
}

impl Default for CanvasOptions {
    fn default() -> Self {
        Self {
            workspace_color: [245, 245, 245, 255],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskOverlay {
    pub tint: Color,
    pub opacity: f32,
}

/// Identifies either a persistent explicit mask layer or an application-owned
/// transient scratch plane. Scratch identifiers have no semantic meaning to
/// the canvas.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaskTarget {
    Layer(ElementId),
    Scratch(u64),
}

impl MaskOverlay {
    #[must_use]
    pub const fn new(tint: Color, opacity: f32) -> Self {
        Self { tint, opacity }
    }
}

/// Viewport-sized state. `size` and camera translations use physical pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub size: PhysicalSize,
    pub camera: Camera,
}

/// One transient frame produced while an element transform is active.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementPreview {
    pub element: ElementId,
    pub frame: Frame,
    pub geometry: Geometry,
}

/// Absolute page-space frame supplied by the React interaction layer.
///
/// Rust validates every frame and derives the corresponding scene geometry;
/// the web UI remains responsible for hit testing and control semantics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementFrame {
    pub element: ElementId,
    pub frame: Frame,
}

/// Final transform result returned to the application for one atomic commit.
#[derive(Clone, Debug, PartialEq)]
pub struct TransformCommit {
    pub page: PageId,
    pub elements: Vec<ElementPreview>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StrokeMode {
    Paint,
    Erase,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub diameter: f32,
    pub color: Color,
    pub mode: StrokeMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RasterStrokeCommit {
    pub page: PageId,
    pub layer: Option<ElementId>,
    pub mode: StrokeMode,
    pub color: Color,
    pub diameter: f32,
    pub points: Vec<PagePoint>,
}
