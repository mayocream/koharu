use std::{sync::Arc, time::Duration};

use koharu_renderer::PageRenderOptions;
use koharu_scene::{BlobId, ElementId, Frame, PageId};

use crate::{Camera, PhysicalPoint, PhysicalSize};

pub type Color = [u8; 4];

#[derive(Clone)]
pub struct CanvasGpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

#[derive(Clone, Debug)]
pub struct CanvasOptions {
    pub max_decoded_bytes: usize,
    pub workspace_color: Color,
    pub text: PageRenderOptions,
}

impl Default for CanvasOptions {
    fn default() -> Self {
        Self {
            max_decoded_bytes: 512 * 1024 * 1024,
            workspace_color: [245, 245, 245, 255],
            text: PageRenderOptions::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BaseImage {
    #[default]
    Source,
    Clean,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PageView {
    #[default]
    EditableSource,
    EditableClean,
    Rendered,
}

impl PageView {
    #[must_use]
    pub const fn editable(base: BaseImage) -> Self {
        match base {
            BaseImage::Source => Self::EditableSource,
            BaseImage::Clean => Self::EditableClean,
        }
    }

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

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayState {
    pub page: PageView,
    pub show_text: bool,
    pub text_mask: Option<MaskOverlay>,
    pub brush_mask: Option<MaskOverlay>,
    pub transition: Option<Duration>,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            page: PageView::EditableSource,
            show_text: true,
            text_mask: None,
            brush_mask: None,
            transition: Some(Duration::from_millis(180)),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub size: PhysicalSize,
    pub camera: Camera,
    pub display: DisplayState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Guide {
    Horizontal(f64),
    Vertical(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementPreview {
    pub element: ElementId,
    pub frame: Frame,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushCursor {
    pub point: PhysicalPoint,
    pub diameter: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayState {
    pub selected: Vec<ElementId>,
    pub hovered: Option<ElementId>,
    pub guides: Vec<Guide>,
    pub show_text_bounds: bool,
    pub draft: Option<Frame>,
    pub element_previews: Vec<ElementPreview>,
    pub brush_cursor: Option<BrushCursor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Handle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HitTarget {
    Handle { element: ElementId, handle: Handle },
    Element(ElementId),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaskPlane {
    Text,
    Brush,
}

impl MaskPlane {
    #[must_use]
    pub const fn asset(self) -> koharu_scene::PageAsset {
        match self {
            Self::Text => koharu_scene::PageAsset::TextMask,
            Self::Brush => koharu_scene::PageAsset::BrushMask,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Brush => "brush",
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
    pub mode: StrokeMode,
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

    pub(crate) fn element(page: PageId, element: ElementId, message: impl Into<String>) -> Self {
        Self {
            page: Some(page),
            element: Some(element),
            blob: None,
            message: message.into(),
        }
    }
}
