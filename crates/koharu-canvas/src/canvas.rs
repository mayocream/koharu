use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use koharu_renderer::{Frame as RendererFrame, ImageKind, LayerKind, Presentation};
use koharu_scene::Revision;
use vello::{
    Scene,
    kurbo::{Affine, Rect},
    peniko::{Color as VelloColor, Compose, Fill, Mix},
};

use crate::{
    ActiveStroke, ActiveTransform, Brush, CanvasGpu, CanvasOptions, ElementFrame, ElementId, Error,
    GpuRenderer, MaskCommit, MaskOverlay, MaskState, MaskTarget, PageId, PagePoint, PhysicalPoint,
    PhysicalSize, RasterStrokeCommit, Result, StrokeMode, TransformCommit, TransformState,
    ViewState,
    damage::RenderDamage,
    raster::{RasterStrokeEdit, RasterStrokeState},
    transform::element_frame,
};

const MAX_BRUSH_DIAMETER: f32 = 128.0;
const MAX_PAGE_DIMENSION: u32 = 32_768;
const MAX_PAGE_PIXELS: u64 = 268_435_456;

pub struct CanvasFrame<'a> {
    /// Final pixels for the viewport, or `None` for a zero-sized viewport.
    pub texture: Option<&'a vello::wgpu::TextureView>,
    pub size: PhysicalSize,
    /// Changes only after newly composed pixels are submitted.
    pub generation: u64,
    /// True only while nonblocking GPU work needs another frame poll.
    pub needs_redraw: bool,
}

struct LocalMask {
    overlay: MaskOverlay,
    state: MaskState,
    pending: Option<(u64, Revision)>,
}

enum ActiveEdit {
    Mask(ActiveStroke),
    Raster(RasterStrokeState),
    Transform(TransformState),
}

impl ActiveEdit {
    fn transform(&self) -> Option<&ActiveTransform> {
        match self {
            Self::Transform(transform) => Some(transform.edit()),
            Self::Mask(_) | Self::Raster(_) => None,
        }
    }
}

/// Interactive viewport over one immutable renderer frame.
pub struct Canvas {
    gpu: GpuRenderer,
    options: CanvasOptions,
    view: ViewState,
    frame: Option<RendererFrame>,
    opacity_overrides: HashMap<ElementId, f32>,
    masks: BTreeMap<MaskTarget, LocalMask>,
    edit: Option<ActiveEdit>,
    damage: RenderDamage,
    generation: u64,
}

impl Canvas {
    pub fn new(gpu: CanvasGpu, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        Self::new_with(gpu, CanvasOptions::default(), wake)
    }

    pub fn new_with(
        gpu: CanvasGpu,
        options: CanvasOptions,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let view = ViewState::default();
        let gpu = GpuRenderer::new(gpu, view.size, wake)?;
        Ok(Self {
            gpu,
            options,
            view,
            frame: None,
            opacity_overrides: HashMap::new(),
            masks: BTreeMap::new(),
            edit: None,
            damage: RenderDamage::initial(),
            generation: 0,
        })
    }

    /// Installs a complete immutable renderer result. The previous valid frame
    /// remains installed if validation fails.
    #[tracing::instrument(level = "info", skip_all, fields(page = %frame.page(), revision = %frame.revision()))]
    pub fn set_frame(&mut self, frame: RendererFrame) -> Result<()> {
        let (width, height) = frame.size();
        if frame.origin() != (0, 0)
            || width == 0
            || height == 0
            || width > MAX_PAGE_DIMENSION
            || height > MAX_PAGE_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_PAGE_PIXELS
        {
            return Err(Error::Invalid(format!(
                "canvas requires a complete page frame at origin 0,0 within surface limits; got origin {:?}, size {width}x{height}",
                frame.origin()
            )));
        }
        if self.frame.as_ref().is_some_and(|current| {
            current.page() == frame.page() && current.revision() >= frame.revision()
        }) {
            return Ok(());
        }
        let page_changed = self.page_id().is_some_and(|page| page != frame.page());
        let size_changed = self
            .page_size()
            .is_some_and(|size| size.width != frame.size().0 || size.height != frame.size().1);
        let clear_transform = matches!(
            self.edit.as_ref(),
            Some(ActiveEdit::Transform(transform)) if transform.clears_for_frame(frame.revision())
        );
        if page_changed || size_changed {
            self.masks.clear();
            self.edit = None;
        } else if matches!(
            self.edit.as_ref(),
            Some(ActiveEdit::Raster(RasterStrokeState::Waiting { revision, .. }))
                if *revision <= frame.revision()
        ) || clear_transform
        {
            // Active transforms are based on one immutable frame. Committed
            // transform and raster previews remain until their acknowledged
            // replacement revision is prepared.
            self.edit = None;
        }
        self.masks.retain(|_, mask| {
            !matches!(mask.pending, Some((generation, revision))
                if generation == mask.state.generation() && revision <= frame.revision())
        });
        self.opacity_overrides.clear();
        self.frame = Some(frame);
        self.damage.content();
        Ok(())
    }

    pub fn clear(&mut self) {
        self.gpu.cancel_samples();
        self.frame = None;
        self.opacity_overrides.clear();
        self.masks.clear();
        self.edit = None;
        self.generation = 0;
        self.damage.content();
    }

    #[must_use]
    pub const fn frame(&self) -> Option<&RendererFrame> {
        self.frame.as_ref()
    }

    #[must_use]
    pub fn page_id(&self) -> Option<PageId> {
        self.frame.as_ref().map(RendererFrame::page)
    }

    #[must_use]
    pub fn revision(&self) -> Option<Revision> {
        self.frame.as_ref().map(RendererFrame::revision)
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[koharu_renderer::RenderDiagnostic] {
        self.frame.as_ref().map_or(&[], |frame| frame.diagnostics())
    }

    #[must_use]
    pub fn page_size(&self) -> Option<PhysicalSize> {
        self.frame
            .as_ref()
            .map(|frame| PhysicalSize::new(frame.size().0, frame.size().1))
    }

    pub fn set_view(&mut self, view: ViewState) {
        if self.view.size != view.size {
            self.damage.target();
        }
        if self.view.camera != view.camera {
            self.damage.content();
        }
        self.view = view;
    }

    pub fn set_camera(&mut self, camera: crate::Camera) {
        if self.view.camera != camera {
            self.view.camera = camera;
            self.damage.content();
        }
    }

    #[must_use]
    pub const fn view(&self) -> &ViewState {
        &self.view
    }

    #[must_use]
    pub const fn camera(&self) -> crate::Camera {
        self.view.camera
    }

    pub fn set_workspace_color(&mut self, color: [u8; 4]) {
        if self.options.workspace_color != color {
            self.options.workspace_color = color;
            self.damage.content();
        }
    }

    pub fn preview_opacity(&mut self, element: ElementId, opacity: Option<f32>) -> Result<()> {
        let frame = self.frame.as_ref().ok_or(Error::NoFrame)?;
        let layer = frame.layer(element).ok_or_else(|| {
            Error::Invalid("opacity preview target is not in the active renderer frame".into())
        })?;
        let changed = match opacity {
            Some(opacity) => {
                if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                    return Err(Error::Invalid(
                        "opacity preview must be finite and between 0 and 1".into(),
                    ));
                }
                if opacity == layer.presentation().opacity {
                    self.opacity_overrides.remove(&element).is_some()
                } else {
                    self.opacity_overrides.insert(element, opacity) != Some(opacity)
                }
            }
            None => self.opacity_overrides.remove(&element).is_some(),
        };
        if changed {
            self.damage.content();
        }
        Ok(())
    }

    #[must_use]
    pub fn screen_to_page(&self, point: PhysicalPoint) -> Option<PagePoint> {
        let point = self.view.camera.screen_to_page(point);
        self.contains_page_point(point).then_some(point)
    }

    #[must_use]
    pub fn page_to_screen(&self, point: PagePoint) -> PhysicalPoint {
        self.view.camera.page_to_screen(point)
    }

    pub fn element_frames(&self) -> Vec<ElementFrame> {
        let Some(frame) = &self.frame else {
            return Vec::new();
        };
        frame
            .layers()
            .iter()
            .filter(|layer| layer.entity() != frame.page())
            .filter(|layer| {
                let presentation = layer.presentation();
                presentation.visible && presentation.opacity > 0.0
            })
            .filter_map(|layer| {
                element_frame(layer).map(|frame| ElementFrame {
                    element: layer.entity(),
                    frame,
                })
            })
            .collect()
    }

    pub fn begin_transform(&mut self, controls: &[ElementFrame]) -> Result<()> {
        if self.edit.is_some() {
            return Err(Error::Invalid(
                "an element transform cannot start during another canvas edit".into(),
            ));
        }
        let frame = self.frame.as_ref().ok_or(Error::NoFrame)?;
        self.edit = Some(ActiveEdit::Transform(TransformState::Active(
            ActiveTransform::new(frame, controls)?,
        )));
        Ok(())
    }

    pub fn update_transform(&mut self, sequence: u64, elements: &[ElementFrame]) -> Result<()> {
        let Some(ActiveEdit::Transform(TransformState::Active(transform))) = self.edit.as_mut()
        else {
            return Err(Error::NoTransform);
        };
        if transform.update(sequence, elements)? {
            self.damage.content();
        }
        Ok(())
    }

    pub fn finish_transform(&mut self) -> Result<Option<TransformCommit>> {
        let edit = self.edit.take();
        let Some(ActiveEdit::Transform(TransformState::Active(transform))) = edit else {
            self.edit = edit;
            return Err(Error::NoTransform);
        };
        let commit = transform.finish();
        if commit.is_some() {
            self.edit = Some(ActiveEdit::Transform(TransformState::Finishing(transform)));
        }
        Ok(commit)
    }

    pub fn acknowledge_transform_commit(&mut self, page: PageId, revision: Revision) -> Result<()> {
        if self.page_id() != Some(page) {
            return Err(Error::Invalid(
                "transform replacement page must match the displayed page".into(),
            ));
        }
        if self.revision().is_some_and(|current| revision <= current) {
            return Err(Error::Invalid(
                "transform replacement revision must follow the displayed frame".into(),
            ));
        }
        let edit = self.edit.take();
        let Some(ActiveEdit::Transform(TransformState::Finishing(transform))) = edit else {
            self.edit = edit;
            return Err(Error::NoTransform);
        };
        self.edit = Some(ActiveEdit::Transform(TransformState::Waiting {
            edit: transform,
            revision,
        }));
        Ok(())
    }

    pub fn cancel_transform(&mut self) {
        if matches!(self.edit.as_ref(), Some(ActiveEdit::Transform(_))) {
            let changed = matches!(self.edit.as_ref(), Some(ActiveEdit::Transform(edit)) if edit.edit().is_changed());
            self.edit = None;
            if changed {
                self.damage.content();
            }
        }
    }

    pub fn begin_raster_stroke(
        &mut self,
        layer: Option<ElementId>,
        brush: Brush,
        point: PagePoint,
    ) -> Result<()> {
        if self.edit.is_some() {
            return Err(Error::Invalid(
                "raster painting cannot start during another canvas edit".into(),
            ));
        }
        validate_brush(brush)?;
        if !self.contains_page_point(point) {
            return Err(Error::Invalid(
                "raster stroke must begin inside the page".into(),
            ));
        }
        let frame = self.frame.as_ref().ok_or(Error::NoFrame)?;
        if let Some(layer) = layer {
            let layer = frame.layer(layer).ok_or_else(|| {
                Error::Invalid("raster target is not in the active renderer frame".into())
            })?;
            if !matches!(
                layer.kind(),
                LayerKind::Image(image)
                    if matches!(image.kind, ImageKind::Cleanup | ImageKind::Paint)
            ) {
                return Err(Error::Invalid(
                    "raster target is not an editable raster layer".into(),
                ));
            }
        } else if brush.mode == StrokeMode::Erase {
            return Err(Error::Invalid(
                "eraser requires an editable raster layer target".into(),
            ));
        }
        let commit = RasterStrokeCommit {
            page: frame.page(),
            layer,
            mode: brush.mode,
            color: brush.color,
            diameter: brush.diameter,
            points: vec![point],
        };
        self.edit = Some(ActiveEdit::Raster(RasterStrokeState::Active(
            RasterStrokeEdit::new(commit),
        )));
        self.damage.content();
        Ok(())
    }

    pub fn extend_raster_stroke(&mut self, points: &[PagePoint]) -> Result<()> {
        let size = self.page_size().ok_or(Error::NoFrame)?;
        let zoom = self.view.camera.zoom().max(f64::EPSILON);
        let Some(ActiveEdit::Raster(RasterStrokeState::Active(edit))) = self.edit.as_mut() else {
            return Err(Error::NoStroke);
        };
        let mut changed = false;
        for point in points {
            if !point.x.is_finite() || !point.y.is_finite() {
                return Err(Error::Invalid("drawing points must be finite".into()));
            }
            let point = PagePoint::new(
                point.x.clamp(0.0, f64::from(size.width)),
                point.y.clamp(0.0, f64::from(size.height)),
            );
            if edit
                .commit
                .points
                .last()
                .is_none_or(|last| (last.x - point.x).hypot(last.y - point.y) >= 0.25 / zoom)
            {
                edit.push_point(point);
                changed = true;
            }
        }
        if changed {
            self.damage.content();
        }
        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all)]
    pub fn finish_raster_stroke(&mut self) -> Result<RasterStrokeCommit> {
        let edit = match self.edit.take() {
            Some(ActiveEdit::Raster(RasterStrokeState::Active(edit))) => edit,
            other => {
                self.edit = other;
                return Err(Error::NoStroke);
            }
        };
        let commit = edit.commit.clone();
        self.edit = Some(ActiveEdit::Raster(RasterStrokeState::Finishing(edit)));
        Ok(commit)
    }

    /// Confirms the application accepted a raster commit. The retained preview
    /// remains visible until a fully prepared replacement frame is installed.
    pub fn acknowledge_raster_commit(&mut self, page: PageId, revision: Revision) -> Result<()> {
        if self.page_id() != Some(page) {
            return Err(Error::Invalid(
                "raster commit belongs to a different page".into(),
            ));
        }
        if self.revision().is_some_and(|current| revision <= current) {
            return Err(Error::Invalid(
                "raster replacement revision must follow the displayed frame".into(),
            ));
        }
        let edit = match self.edit.take() {
            Some(ActiveEdit::Raster(RasterStrokeState::Finishing(edit))) => edit,
            other => {
                self.edit = other;
                return Err(Error::NoStroke);
            }
        };
        self.edit = Some(ActiveEdit::Raster(RasterStrokeState::Waiting {
            edit,
            revision,
        }));
        Ok(())
    }

    pub fn cancel_raster_stroke(&mut self) {
        if matches!(self.edit, Some(ActiveEdit::Raster(_))) {
            self.edit = None;
            self.damage.content();
        }
    }

    pub fn begin_mask_stroke(
        &mut self,
        target: MaskTarget,
        overlay: MaskOverlay,
        brush: Brush,
        point: PagePoint,
    ) -> Result<()> {
        if matches!(target, MaskTarget::Layer(_)) {
            return Err(Error::Invalid(
                "renderer layers do not yet expose editable mask pixels".into(),
            ));
        }
        if self.edit.is_some() {
            return Err(Error::Invalid(
                "mask painting cannot start during another canvas edit".into(),
            ));
        }
        validate_brush(brush)?;
        if !overlay.opacity.is_finite() || !(0.0..=1.0).contains(&overlay.opacity) {
            return Err(Error::Invalid(
                "mask overlay opacity must be finite and between 0 and 1".into(),
            ));
        }
        if !self.contains_page_point(point) {
            return Err(Error::Invalid(
                "mask stroke must begin inside the page".into(),
            ));
        }
        self.frame.as_ref().ok_or(Error::NoFrame)?;
        let size = self.page_size().expect("renderer frame exists above");
        let local = self.masks.entry(target).or_insert_with(|| LocalMask {
            overlay,
            state: MaskState::empty(size),
            pending: None,
        });
        local.overlay = overlay;
        local.pending = None;
        let mut stroke = ActiveStroke::new(target, brush, point);
        let dirty = stroke.paint(&mut local.state, point, point);
        stroke.dirty = dirty;
        self.edit = Some(ActiveEdit::Mask(stroke));
        if !dirty.is_empty() {
            self.damage.content();
        }
        Ok(())
    }

    pub fn extend_mask_stroke(&mut self, target: MaskTarget, points: &[PagePoint]) -> Result<()> {
        let size = self.page_size().ok_or(Error::NoFrame)?;
        let Some(ActiveEdit::Mask(stroke)) = self.edit.as_mut() else {
            return Err(Error::NoStroke);
        };
        if stroke.target != target {
            return Err(Error::Invalid("mask stroke targets another mask".into()));
        }
        let mask = &mut self
            .masks
            .get_mut(&target)
            .expect("active mask state exists")
            .state;
        let mut changed = false;
        for point in points {
            if !point.x.is_finite() || !point.y.is_finite() {
                return Err(Error::Invalid("drawing points must be finite".into()));
            }
            let point = PagePoint::new(
                point.x.clamp(0.0, f64::from(size.width)),
                point.y.clamp(0.0, f64::from(size.height)),
            );
            let dirty = stroke.paint(mask, stroke.last, point);
            stroke.last = point;
            if !dirty.is_empty() {
                stroke.dirty = stroke.dirty.union(dirty);
                changed = true;
            }
        }
        if changed {
            self.damage.content();
        }
        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all)]
    pub fn finish_mask_stroke(&mut self, target: MaskTarget) -> Result<Option<MaskCommit>> {
        let stroke = match self.edit.take() {
            Some(ActiveEdit::Mask(stroke)) if stroke.target == target => stroke,
            other => {
                self.edit = other;
                return Err(Error::NoStroke);
            }
        };
        let page = self.page_id().ok_or(Error::NoFrame)?;
        if stroke.dirty.is_empty() {
            return Ok(None);
        }
        let commit = self
            .masks
            .get_mut(&target)
            .expect("active mask state exists")
            .state
            .finish(page, target, stroke.dirty);
        Ok((!commit.dirty.is_empty()).then_some(commit))
    }

    pub fn cancel_mask_stroke(&mut self, target: MaskTarget) -> Result<()> {
        let stroke = match self.edit.take() {
            Some(ActiveEdit::Mask(stroke)) if stroke.target == target => stroke,
            other => {
                self.edit = other;
                return Err(Error::NoStroke);
            }
        };
        let changed = !stroke.dirty.is_empty();
        stroke.restore(
            &mut self
                .masks
                .get_mut(&target)
                .expect("active mask state exists")
                .state,
        );
        if changed {
            self.damage.content();
        }
        Ok(())
    }

    pub fn clear_mask(&mut self, target: MaskTarget) {
        if self.masks.remove(&target).is_some() {
            if matches!(self.edit, Some(ActiveEdit::Mask(ref stroke)) if stroke.target == target) {
                self.edit = None;
            }
            self.damage.content();
        }
    }

    /// Keeps a committed mask visible until the renderer frame containing its
    /// replacement pixels is installed.
    pub fn acknowledge_mask_commit(
        &mut self,
        page: PageId,
        target: MaskTarget,
        generation: u64,
        revision: Revision,
    ) -> Result<()> {
        if self.page_id() != Some(page) {
            return Err(Error::Invalid(
                "mask commit belongs to a different page".into(),
            ));
        }
        if self.revision().is_some_and(|current| revision <= current) {
            return Err(Error::Invalid(
                "mask replacement revision must follow the displayed frame".into(),
            ));
        }
        let mask = self
            .masks
            .get_mut(&target)
            .ok_or_else(|| Error::Invalid("mask target no longer exists".into()))?;
        if mask.state.generation() != generation {
            return Err(Error::Invalid("mask commit generation is stale".into()));
        }
        mask.pending = Some((generation, revision));
        Ok(())
    }

    #[must_use]
    pub fn needs_redraw(&self) -> bool {
        self.damage.target_pending() || self.damage.content_pending() || self.gpu.samples_pending()
    }

    pub fn render(&mut self) -> Result<CanvasFrame<'_>> {
        self.gpu.poll_samples();
        if self.damage.target_pending() {
            self.gpu.resize(self.view.size);
            self.damage.clear_target();
        }
        if self.damage.content_pending() {
            if !self.view.size.is_empty() {
                let scene = self.build_scene();
                self.gpu
                    .render_content(&scene, self.options.workspace_color)?;
                self.generation = self.generation.wrapping_add(1).max(1);
            }
            self.damage.clear_content();
        }
        Ok(CanvasFrame {
            texture: self.gpu.output(),
            size: self.view.size,
            generation: self.generation,
            needs_redraw: self.gpu.samples_pending(),
        })
    }

    /// Queues a read from the last successfully rendered viewport image.
    pub fn sample_color(
        &mut self,
        point: PhysicalPoint,
        complete: impl FnOnce(Result<[u8; 4]>) + Send + 'static,
    ) -> Result<()> {
        if self.generation == 0 || self.damage.target_pending() {
            return Err(Error::Invalid(
                "cannot sample before the current viewport has rendered".into(),
            ));
        }
        self.gpu.request_pixel(point.x, point.y, complete)
    }

    fn contains_page_point(&self, point: PagePoint) -> bool {
        self.page_size().is_some_and(|size| {
            point.x.is_finite()
                && point.y.is_finite()
                && point.x >= 0.0
                && point.y >= 0.0
                && point.x <= f64::from(size.width)
                && point.y <= f64::from(size.height)
        })
    }

    fn build_scene(&mut self) -> Scene {
        let mut scene = Scene::new();
        let Some(frame) = self.frame.as_ref() else {
            return scene;
        };
        let mut page_scene = Scene::new();
        let size = frame.size();
        let page_rect = Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
        let active_transform = self.edit.as_ref().and_then(ActiveEdit::transform);
        let erase = match self.edit.as_ref() {
            Some(ActiveEdit::Raster(stroke)) if stroke.edit().commit.mode == StrokeMode::Erase => {
                Some(stroke.edit())
            }
            _ => None,
        };
        if active_transform.is_some() || !self.opacity_overrides.is_empty() || erase.is_some() {
            let origin = frame.origin();
            let normalize = Affine::translate((-f64::from(origin.0), -f64::from(origin.1)));
            for layer in frame.layers() {
                let transform = normalize
                    * active_transform
                        .and_then(|transform| transform.affine(layer.entity()))
                        .unwrap_or(Affine::IDENTITY);
                let mut presentation = layer.presentation();
                if let Some(opacity) = self.opacity_overrides.get(&layer.entity()) {
                    presentation = Presentation {
                        opacity: *opacity,
                        ..presentation
                    };
                }
                if erase.and_then(|edit| edit.commit.layer) == Some(layer.entity()) {
                    let mut erased_layer = Scene::new();
                    // Vello luminance masks must be isolated before a scene is
                    // presented to a surface that does not preserve alpha.
                    erased_layer.push_layer(
                        Fill::NonZero,
                        Compose::SrcOver,
                        1.0,
                        Affine::IDENTITY,
                        &page_rect,
                    );
                    layer.append_with_presentation(
                        &mut erased_layer,
                        Some(transform),
                        presentation,
                    );
                    erased_layer.push_luminance_mask_layer(
                        Fill::NonZero,
                        1.0,
                        Affine::IDENTITY,
                        &page_rect,
                    );
                    erased_layer.fill(
                        Fill::NonZero,
                        Affine::IDENTITY,
                        VelloColor::from_rgba8(255, 255, 255, 255),
                        None,
                        &page_rect,
                    );
                    erased_layer.append(
                        &erase.expect("erase edit exists in this branch").preview,
                        None,
                    );
                    erased_layer.pop_layer();
                    erased_layer.pop_layer();
                    page_scene.append(&erased_layer, None);
                } else {
                    layer.append_with_presentation(&mut page_scene, Some(transform), presentation);
                }
            }
        } else {
            frame.append_to(&mut page_scene, None);
        }

        if let Some(ActiveEdit::Raster(stroke)) = self.edit.as_ref()
            && stroke.edit().commit.mode == StrokeMode::Paint
        {
            let opacity = f32::from(stroke.edit().commit.color[3]) / 255.0;
            if opacity > 0.0 {
                if opacity < 1.0 {
                    let size = frame.size();
                    page_scene.push_layer(
                        Fill::NonZero,
                        Mix::Normal,
                        opacity,
                        Affine::IDENTITY,
                        &Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1)),
                    );
                }
                page_scene.append(&stroke.edit().preview, None);
                if opacity < 1.0 {
                    page_scene.pop_layer();
                }
            }
        }
        for mask in self.masks.values_mut() {
            mask.state
                .for_each_tinted_tile(mask.overlay, |x, y, image| {
                    page_scene.draw_image(image, Affine::translate((f64::from(x), f64::from(y))));
                });
        }

        let viewport_rect = Rect::new(
            0.0,
            0.0,
            f64::from(self.view.size.width),
            f64::from(self.view.size.height),
        );
        let camera = self.view.camera.affine();
        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &viewport_rect);
        scene.push_clip_layer(Fill::NonZero, camera, &page_rect);
        scene.append(&page_scene, Some(camera));
        scene.pop_layer();
        scene.pop_layer();
        scene
    }
}

fn validate_brush(brush: Brush) -> Result<()> {
    if !brush.diameter.is_finite() || brush.diameter <= 0.0 || brush.diameter > MAX_BRUSH_DIAMETER {
        return Err(Error::Invalid(format!(
            "brush diameter must be finite and in (0, {MAX_BRUSH_DIAMETER}]"
        )));
    }
    Ok(())
}
