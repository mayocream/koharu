use anyhow::Result;
use async_trait::async_trait;
use koharu_protocol::{CanvasState, PaintBrush, Point, TransformFrame};
use koharu_renderer::{FontFamily, Frame};
use koharu_scene::{EntityId, Geometry, Revision, Snapshot};

/// An immutable application render handed to the sole desktop presenter.
///
/// The receiver may update canvas textures but cannot mutate application or
/// scene state. Desktop surface ownership is intentionally absent.
#[derive(Clone)]
pub enum PresentationUpdate {
    Frame { frame: Frame, view: ViewDisposition },
    Clear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewDisposition {
    Preserve,
    Fit,
}

#[async_trait]
pub trait Presentation: Send + Sync {
    async fn apply(&self, update: PresentationUpdate) -> Result<CanvasState>;
    async fn canvas(&self, operation: CanvasOperation) -> Result<CanvasOutput>;
}

#[derive(Clone, Debug)]
pub enum CanvasOperation {
    SetZoom(f32),
    SetView {
        zoom: f64,
        translation: [f64; 2],
    },
    Fit {
        page_size: (u32, u32),
    },
    BeginPaint {
        layer: Option<EntityId>,
        point: Point,
        brush: PaintBrush,
    },
    BeginErase {
        layer: EntityId,
        point: Point,
        diameter: f32,
    },
    ExtendRaster(Vec<Point>),
    FinishRaster,
    CancelRaster,
    BeginTransform(Vec<TransformFrame>),
    UpdateTransform {
        frame: u64,
        elements: Vec<TransformFrame>,
    },
    PreviewOpacity {
        element: EntityId,
        opacity: Option<f32>,
    },
    FinishTransform,
    CancelTransform,
    BeginInpaint {
        point: Point,
        diameter: f32,
    },
    ExtendInpaint(Vec<Point>),
    FinishInpaint,
    CancelInpaint,
    SampleColor(Point),
    SetViewport {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        dpr: f64,
        background: [u8; 3],
        fitted_page: Option<(u32, u32)>,
    },
    AcknowledgeRaster {
        page: EntityId,
        revision: Revision,
    },
    AcknowledgeTransform {
        page: EntityId,
        revision: Revision,
    },
}

pub enum CanvasOutput {
    Unit,
    State(CanvasState),
    Raster(RasterStroke),
    Transform(Option<TransformCommit>),
    Inpaint(Option<InpaintCommit>),
    Color([u8; 4]),
}

pub struct RasterStroke {
    pub page: EntityId,
    pub layer: Option<EntityId>,
    pub mode: koharu_canvas::StrokeMode,
    pub color: [u8; 4],
    pub diameter: f32,
    pub points: Vec<Point>,
}

pub struct TransformCommit {
    pub page: EntityId,
    pub elements: Vec<(EntityId, Geometry)>,
}

pub struct InpaintCommit {
    pub mask: koharu_pipeline::InpaintingMask,
    pub bounds: koharu_pipeline::Bounds,
}

/// Semantic page rendering owned by the application, separate from desktop
/// texture and surface presentation.
#[async_trait]
pub trait PageRenderer: Send + Sync {
    async fn render(&self, snapshot: &Snapshot, page: EntityId) -> Result<Frame>;
    async fn rasterize(&self, frame: &Frame) -> Result<image::RgbaImage>;
    async fn export_psd(&self, snapshot: &Snapshot, frame: &Frame) -> Result<Vec<u8>>;
    async fn available_fonts(&self) -> Result<Vec<FontFamily>>;
    async fn font_preview(&self, family_name: &str) -> Result<Vec<u8>>;
    fn discard_retained_nodes(&self);
}

#[async_trait]
impl PageRenderer for koharu_renderer::Renderer {
    async fn render(&self, snapshot: &Snapshot, page: EntityId) -> Result<Frame> {
        Ok(koharu_renderer::Renderer::render(self, snapshot, page).await?)
    }

    async fn rasterize(&self, frame: &Frame) -> Result<image::RgbaImage> {
        Ok(koharu_renderer::Renderer::rasterize(
            self,
            frame,
            koharu_renderer::RasterOptions::default(),
        )
        .await?
        .image)
    }

    async fn export_psd(&self, snapshot: &Snapshot, frame: &Frame) -> Result<Vec<u8>> {
        Ok(koharu_psd::export_page(
            self,
            snapshot,
            frame,
            &koharu_psd::PsdExportOptions::default(),
        )
        .await?)
    }

    async fn available_fonts(&self) -> Result<Vec<FontFamily>> {
        Ok(koharu_renderer::Renderer::available_fonts(self).await?)
    }

    async fn font_preview(&self, family_name: &str) -> Result<Vec<u8>> {
        Ok(koharu_renderer::Renderer::font_preview(self, family_name).await?)
    }

    fn discard_retained_nodes(&self) {
        koharu_renderer::Renderer::discard_retained_nodes(self);
    }
}
