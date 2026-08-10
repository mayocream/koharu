use std::sync::Arc;

use koharu_scene::{EntityId, Geometry};
use vello::wgpu;

use crate::{Camera, Frame, PagePoint, PhysicalSize};

pub type PageId = EntityId;
pub type ElementId = EntityId;
pub type Color = [u8; 4];

/// Host-created GPU objects shared with the desktop presenter.
#[derive(Clone)]
pub struct CanvasGpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

/// Canvas-owned presentation policy. Document rendering policy belongs to the renderer.
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

impl MaskOverlay {
    #[must_use]
    pub const fn new(tint: Color, opacity: f32) -> Self {
        Self { tint, opacity }
    }
}

/// Scratch masks are application-scoped transient workspaces and never enter
/// a renderer frame or document cache. `Layer` reserves the stable target
/// shape for scene layers once renderer metadata exposes editable mask pixels.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaskTarget {
    Layer(ElementId),
    Scratch(u64),
}

/// Viewport-sized state. `size` and camera translations use physical pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub size: PhysicalSize,
    pub camera: Camera,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElementPreview {
    pub element: ElementId,
    pub frame: Frame,
    pub geometry: Geometry,
}

/// Absolute page-space frame supplied by the React interaction layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementFrame {
    pub element: ElementId,
    pub frame: Frame,
}

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
