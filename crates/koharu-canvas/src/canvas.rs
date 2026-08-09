use std::{collections::HashMap, sync::Arc};

use koharu_renderer::{Composition, LayerKind, Presentation};
use koharu_scene::{PixelFormat, Revision};
use vello::{
    Scene,
    kurbo::{Affine, BezPath, Circle, Rect, Stroke},
    peniko::{Color as VelloColor, Fill, Mix},
};

use crate::{
    ActiveStroke, ActiveTransform, Brush, CanvasGpu, CanvasOptions, ElementFrame, ElementId, Error,
    GpuRenderer, MaskCommit, MaskOverlay, MaskState, MaskTarget, PageId, PagePoint, PhysicalPoint,
    PhysicalSize, RasterStrokeCommit, Result, StrokeMode, TransformCommit, ViewState,
    damage::RenderDamage, transform::element_frame,
};

const MAX_BRUSH_DIAMETER: f32 = 128.0;
const MAX_SURFACE_DIMENSION: u32 = 32_768;
const MAX_SURFACE_PIXELS: u64 = 268_435_456;

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
}

struct RasterStrokeEdit {
    commit: RasterStrokeCommit,
    preview: Scene,
}

impl RasterStrokeEdit {
    fn new(commit: RasterStrokeCommit) -> Self {
        let mut preview = Scene::new();
        if commit.mode == StrokeMode::Paint {
            draw_freehand_dot(
                &mut preview,
                commit.points[0],
                commit.diameter,
                commit.color,
            );
        }
        Self { commit, preview }
    }

    fn push_point(&mut self, point: PagePoint) {
        let previous = *self
            .commit
            .points
            .last()
            .expect("a raster edit always has its initial point");
        if self.commit.mode == StrokeMode::Paint {
            draw_freehand_segment(
                &mut self.preview,
                previous,
                point,
                self.commit.diameter,
                self.commit.color,
            );
        }
        self.commit.points.push(point);
    }
}

enum RasterStrokeState {
    Active(RasterStrokeEdit),
    Finishing(RasterStrokeEdit),
}

impl RasterStrokeState {
    fn edit(&self) -> &RasterStrokeEdit {
        match self {
            Self::Active(edit) | Self::Finishing(edit) => edit,
        }
    }
}

enum ActiveEdit {
    Mask(ActiveStroke),
    Raster(RasterStrokeState),
    Transform(ActiveTransform),
}

impl ActiveEdit {
    fn transform(&self) -> Option<&ActiveTransform> {
        match self {
            Self::Transform(transform) => Some(transform),
            Self::Mask(_) | Self::Raster(_) => None,
        }
    }
}

/// Interactive viewport over one immutable renderer composition.
pub struct Canvas {
    gpu: GpuRenderer,
    options: CanvasOptions,
    view: ViewState,
    composition: Option<Composition>,
    retained: Scene,
    opacity_overrides: HashMap<ElementId, f32>,
    masks: HashMap<MaskTarget, LocalMask>,
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
            composition: None,
            retained: Scene::new(),
            opacity_overrides: HashMap::new(),
            masks: HashMap::new(),
            edit: None,
            damage: RenderDamage::initial(),
            generation: 0,
        })
    }

    /// Installs a complete immutable renderer result. No scene traversal,
    /// resource loading, or persistent node construction occurs here.
    pub fn set_composition(&mut self, composition: Composition) -> Result<()> {
        let (width, height) = composition.size();
        if width == 0
            || height == 0
            || width > MAX_SURFACE_DIMENSION
            || height > MAX_SURFACE_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_SURFACE_PIXELS
        {
            return Err(Error::Invalid(format!(
                "composition surface {width}x{height} exceeds canvas limits"
            )));
        }
        let mut retained = Scene::new();
        composition.append_to(&mut retained, None);
        self.gpu.cancel_samples();
        self.composition = Some(composition);
        self.retained = retained;
        self.opacity_overrides.clear();
        self.masks.clear();
        self.edit = None;
        self.damage.content();
        Ok(())
    }

    pub fn clear(&mut self) {
        self.gpu.cancel_samples();
        self.composition = None;
        self.retained = Scene::new();
        self.opacity_overrides.clear();
        self.masks.clear();
        self.edit = None;
        self.damage.content();
    }

    #[must_use]
    pub const fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }

    #[must_use]
    pub fn page_id(&self) -> Option<PageId> {
        self.composition.as_ref().map(Composition::page)
    }

    #[must_use]
    pub fn revision(&self) -> Option<Revision> {
        self.composition.as_ref().map(Composition::revision)
    }

    #[must_use]
    pub fn page_size(&self) -> Option<PhysicalSize> {
        self.composition
            .as_ref()
            .map(|composition| PhysicalSize::new(composition.size().0, composition.size().1))
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
        let composition = self.composition.as_ref().ok_or(Error::NoComposition)?;
        let layer = composition.layer(element).ok_or_else(|| {
            Error::Invalid("opacity preview target is not in the active composition".into())
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
        let Some(composition) = &self.composition else {
            return Vec::new();
        };
        composition
            .layers()
            .iter()
            .filter(|layer| layer.entity() != composition.page())
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
        let composition = self.composition.as_ref().ok_or(Error::NoComposition)?;
        self.edit = Some(ActiveEdit::Transform(ActiveTransform::new(
            composition,
            controls,
        )?));
        Ok(())
    }

    pub fn update_transform(&mut self, frame: u64, elements: &[ElementFrame]) -> Result<()> {
        let Some(ActiveEdit::Transform(transform)) = self.edit.as_mut() else {
            return Err(Error::NoTransform);
        };
        if transform.update(frame, elements)? {
            self.damage.content();
        }
        Ok(())
    }

    pub fn finish_transform(&mut self) -> Result<Option<TransformCommit>> {
        let Some(ActiveEdit::Transform(transform)) = self.edit.as_mut() else {
            return Err(Error::NoTransform);
        };
        Ok(transform.finish())
    }

    pub fn cancel_transform(&mut self) {
        if matches!(self.edit, Some(ActiveEdit::Transform(_))) {
            self.edit = None;
            self.damage.content();
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
        let composition = self.composition.as_ref().ok_or(Error::NoComposition)?;
        if let Some(layer) = layer {
            let layer = composition.layer(layer).ok_or_else(|| {
                Error::Invalid("raster target is not in the active composition".into())
            })?;
            if !matches!(
                layer.kind(),
                LayerKind::Pixel(pixel) if pixel.format == PixelFormat::Color
            ) {
                return Err(Error::Invalid(
                    "raster target is not a color pixel layer".into(),
                ));
            }
        }
        let commit = RasterStrokeCommit {
            page: composition.page(),
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
        let size = self.page_size().ok_or(Error::NoComposition)?;
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

    pub fn cancel_raster_stroke(&mut self) {
        if matches!(self.edit, Some(ActiveEdit::Raster(_))) {
            self.edit = None;
            self.damage.content();
        }
    }

    pub fn begin_layer_mask_stroke(
        &mut self,
        layer: ElementId,
        brush: Brush,
        point: PagePoint,
    ) -> Result<()> {
        let composition = self.composition.as_ref().ok_or(Error::NoComposition)?;
        let target = composition
            .layer(layer)
            .ok_or_else(|| Error::Invalid("mask target is not in the active composition".into()))?;
        let tint = match target.kind() {
            LayerKind::Pixel(pixel) => match pixel.format {
                PixelFormat::Mask { tint, .. } => tint,
                PixelFormat::Color => {
                    return Err(Error::Invalid("mask target is a color pixel layer".into()));
                }
            },
            LayerKind::Text(_) => {
                return Err(Error::Invalid("mask target is not a pixel layer".into()));
            }
        };
        self.begin_mask_stroke(
            MaskTarget::Layer(layer),
            MaskOverlay::new(tint, 1.0),
            brush,
            point,
        )
    }

    pub fn begin_scratch_mask_stroke(
        &mut self,
        id: u64,
        overlay: MaskOverlay,
        brush: Brush,
        point: PagePoint,
    ) -> Result<()> {
        if !overlay.opacity.is_finite() {
            return Err(Error::Invalid("mask overlay opacity must be finite".into()));
        }
        self.begin_mask_stroke(MaskTarget::Scratch(id), overlay, brush, point)
    }

    fn begin_mask_stroke(
        &mut self,
        target: MaskTarget,
        overlay: MaskOverlay,
        brush: Brush,
        point: PagePoint,
    ) -> Result<()> {
        if self.edit.is_some() {
            return Err(Error::Invalid(
                "mask painting cannot start during another canvas edit".into(),
            ));
        }
        validate_brush(brush)?;
        if !self.contains_page_point(point) {
            return Err(Error::Invalid(
                "mask stroke must begin inside the page".into(),
            ));
        }
        self.composition.as_ref().ok_or(Error::NoComposition)?;
        let size = self.page_size().expect("composition exists above");
        let local = self.masks.entry(target).or_insert_with(|| LocalMask {
            overlay,
            state: MaskState::empty(size),
        });
        local.overlay = overlay;
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
        let size = self.page_size().ok_or(Error::NoComposition)?;
        let Some(ActiveEdit::Mask(stroke)) = self.edit.as_mut() else {
            return Err(Error::NoStroke);
        };
        if stroke.target != target {
            return Err(Error::Invalid("mask stroke targets another layer".into()));
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

    pub fn finish_mask_stroke(&mut self, target: MaskTarget) -> Result<Option<MaskCommit>> {
        let stroke = match self.edit.take() {
            Some(ActiveEdit::Mask(stroke)) if stroke.target == target => stroke,
            other => {
                self.edit = other;
                return Err(Error::NoStroke);
            }
        };
        let page = self.page_id().ok_or(Error::NoComposition)?;
        let commit = self
            .masks
            .get_mut(&target)
            .expect("active mask state exists")
            .state
            .finish(page, target, stroke.dirty);
        Ok(commit)
    }

    pub fn cancel_mask_stroke(&mut self, target: MaskTarget) -> Result<()> {
        let stroke = match self.edit.take() {
            Some(ActiveEdit::Mask(stroke)) if stroke.target == target => stroke,
            other => {
                self.edit = other;
                return Err(Error::NoStroke);
            }
        };
        stroke.restore(
            &mut self
                .masks
                .get_mut(&target)
                .expect("active mask state exists")
                .state,
        );
        self.damage.content();
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

    #[tracing::instrument(
        skip_all,
        fields(
            width = self.view.size.width,
            height = self.view.size.height,
            target_pending = self.damage.target_pending(),
            content_pending = self.damage.content_pending(),
            generation = self.generation,
        )
    )]
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

    /// Queues a color read from the last successfully rendered viewport image.
    /// Completion never blocks this call or the desktop-state lock.
    pub fn sample_color(
        &mut self,
        point: PhysicalPoint,
        complete: impl FnOnce(Result<[u8; 4]>) + Send + 'static,
    ) -> Result<()> {
        if self.generation == 0 || self.damage.target_pending() || self.damage.content_pending() {
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
        let Some(composition) = self.composition.as_ref() else {
            return scene;
        };
        let mut page_scene = Scene::new();
        let active_transform = self.edit.as_ref().and_then(ActiveEdit::transform);
        if active_transform.is_some() || !self.opacity_overrides.is_empty() {
            let origin = composition.origin();
            let normalize = Affine::translate((-f64::from(origin.0), -f64::from(origin.1)));
            for layer in composition.layers() {
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
                layer.append_with_presentation(&mut page_scene, Some(transform), presentation);
            }
        } else {
            page_scene.append(&self.retained, None);
        }

        if let Some(ActiveEdit::Raster(stroke)) = self.edit.as_ref()
            && stroke.edit().commit.mode == StrokeMode::Paint
        {
            let opacity = f32::from(stroke.edit().commit.color[3]) / 255.0;
            if opacity > 0.0 {
                if opacity < 1.0 {
                    let size = composition.size();
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

        let size = composition.size();
        let page_rect = Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
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

fn draw_freehand_dot(scene: &mut Scene, point: PagePoint, diameter: f32, color: [u8; 4]) {
    let color = VelloColor::from_rgba8(color[0], color[1], color[2], u8::MAX);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color,
        None,
        &Circle::new((point.x, point.y), f64::from(diameter) * 0.5),
    );
}

fn draw_freehand_segment(
    scene: &mut Scene,
    from: PagePoint,
    to: PagePoint,
    diameter: f32,
    color: [u8; 4],
) {
    let color = VelloColor::from_rgba8(color[0], color[1], color[2], u8::MAX);
    let mut path = BezPath::new();
    path.move_to((from.x, from.y));
    path.line_to((to.x, to.y));
    scene.stroke(
        &Stroke::new(f64::from(diameter)),
        Affine::IDENTITY,
        color,
        None,
        &path,
    );
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color,
        None,
        &Circle::new((to.x, to.y), f64::from(diameter) * 0.5),
    );
}
