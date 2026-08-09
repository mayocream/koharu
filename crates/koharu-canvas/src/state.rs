use std::{sync::Arc, time::Duration};

use koharu_renderer::RenderTheme;
use koharu_scene::{BlobId, EntityId, Geometry};
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

/// Memory, workspace, and text-rendering policy for one canvas.
#[derive(Clone, Debug)]
pub struct CanvasOptions {
    pub max_decoded_bytes: usize,
    pub workspace_color: Color,
    pub text: RenderTheme,
}

impl Default for CanvasOptions {
    fn default() -> Self {
        Self {
            max_decoded_bytes: 512 * 1024 * 1024,
            workspace_color: [245, 245, 245, 255],
            text: RenderTheme::default(),
        }
    }
}

/// Selects either editable live layers or the flattened rendered artifact.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PageView {
    #[default]
    Editable,
    Rendered,
}

impl PageView {
    #[must_use]
    pub const fn is_editable(self) -> bool {
        !matches!(self, Self::Rendered)
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

/// Presentation-only choices; changing these never mutates the scene snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayState {
    pub page: PageView,
    pub show_text: bool,
    pub text_mask: Option<MaskOverlay>,
    pub transition: Option<Duration>,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            page: PageView::Editable,
            show_text: true,
            text_mask: None,
            transition: Some(Duration::from_millis(180)),
        }
    }
}

/// Viewport-sized state. `size` and camera translations use physical pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub size: PhysicalSize,
    pub camera: Camera,
    pub display: DisplayState,
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaskPlane {
    Text,
    Inpaint,
}

impl MaskPlane {
    #[must_use]
    pub const fn asset_role(self) -> &'static str {
        match self {
            Self::Text => "text-mask",
            Self::Inpaint => "inpaint",
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Inpaint => "inpaint",
        }
    }
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

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasDiagnostic {
    pub page: Option<PageId>,
    pub element: Option<ElementId>,
    pub blob: Option<BlobId>,
    pub message: String,
}

impl CanvasDiagnostic {
    pub(crate) fn resource(page: Option<PageId>, blob: BlobId, message: impl Into<String>) -> Self {
        Self {
            page,
            element: None,
            blob: Some(blob),
            message: message.into(),
        }
    }
}
