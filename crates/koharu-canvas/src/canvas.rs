use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use koharu_renderer::RenderTheme;
use koharu_scene::{
    BlobId, Change, Component, ComponentOwner, EntityChange, Revision, Snapshot, Typography,
    Visibility,
};
use vello::wgpu;
use vello::{
    Scene,
    kurbo::{Affine, BezPath, Circle, Rect, Stroke, Vec2},
    peniko::{Color as VelloColor, Fill, Mix},
};

use crate::damage::RenderDamage;
use crate::{
    ActiveStroke, ActiveTransform, Brush, CanvasDiagnostic, CanvasGpu, CanvasOptions, CanvasPage,
    ElementFrame, ElementId, ElementSceneContext, ElementScenes, Error, GpuRenderer, MaskCommit,
    MaskPlane, MaskState, PageId, PagePoint, PageView, PhysicalPoint, PhysicalSize,
    RasterStrokeCommit, ResourceEvent, ResourceKind, Resources, Result, StrokeMode,
    TransformCommit,
};

pub struct CanvasFrame<'a> {
    /// Final Vello pixels for the desktop surface.
    pub texture: &'a wgpu::TextureView,
    pub size: PhysicalSize,
    /// Changes only after a new output texture image has been composed.
    pub generation: u64,
    /// True only for bounded animations such as source/clean transitions.
    pub needs_redraw: bool,
}

struct ImageTransition {
    from: BlobId,
    to: BlobId,
    started: Instant,
    duration: Duration,
}

/// Native scene viewport. React owns interaction policy; the desktop host
/// presents the returned texture and invokes semantic preview operations.
pub struct Canvas {
    // GPU details are intentionally hidden behind one backend.
    gpu: GpuRenderer,
    options: CanvasOptions,

    // Authoritative presentation inputs and asynchronously decoded assets.
    resources: Resources,
    view: crate::ViewState,
    render_size: PhysicalSize,
    render_origin: PhysicalPoint,
    render_target_explicit: bool,
    snapshot: Option<Snapshot>,
    page: Option<CanvasPage>,
    revision: Revision,

    // Derived data that may be rebuilt without mutating the scene snapshot.
    masks: HashMap<MaskPlane, MaskState>,
    element_scenes: ElementScenes,
    displayed_base: Option<BlobId>,
    transition: Option<ImageTransition>,
    reported_fallback: Option<(PageId, PageView)>,

    // At most one low-latency editing operation is active for each category.
    stroke: Option<ActiveStroke>,
    raster_stroke: Option<RasterStrokeCommit>,
    transform: Option<ActiveTransform>,

    diagnostics: Vec<CanvasDiagnostic>,
    // Damage is the only authority for deciding which render stages run.
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
        let resources = Resources::new(options.max_decoded_bytes, wake);
        let view = crate::ViewState::default();
        let render_size = view.size;
        let gpu = GpuRenderer::new(gpu, render_size)?;
        Ok(Self {
            gpu,
            options,
            resources,
            view,
            render_size,
            render_origin: PhysicalPoint::default(),
            render_target_explicit: false,
            snapshot: None,
            page: None,
            revision: Revision::ZERO,
            masks: HashMap::new(),
            element_scenes: ElementScenes::new()?,
            displayed_base: None,
            transition: None,
            reported_fallback: None,
            stroke: None,
            raster_stroke: None,
            transform: None,
            diagnostics: Vec::new(),
            damage: RenderDamage::initial(),
            generation: 0,
        })
    }

    pub fn show_page(&mut self, snapshot: &Snapshot, page: PageId) -> Result<()> {
        let next = CanvasPage::load(snapshot, page)?;
        let source_id = next
            .assets
            .source
            .expect("CanvasPage requires a source asset");
        let source = snapshot.read_blob(source_id)?;

        self.snapshot = Some(snapshot.clone());
        self.page = Some(next);
        self.revision = snapshot.revision();
        self.stroke = None;
        self.raster_stroke = None;
        self.transform = None;
        self.masks.clear();
        self.element_scenes.clear();
        self.displayed_base = None;
        self.transition = None;
        self.reported_fallback = None;
        self.masks.insert(
            MaskPlane::Text,
            MaskState::empty(self.page.as_ref().expect("page was set").size),
        );
        self.masks.insert(
            MaskPlane::Inpaint,
            MaskState::empty(self.page.as_ref().expect("page was set").size),
        );
        self.resources
            .request(source_id, ResourceKind::Color, source);
        self.request_page_resources(snapshot);
        // Decoded images and Vello atlas entries have independent lifetimes.
        // Refresh resident pixels whenever a cached page becomes active again.
        let ready_images = self
            .page
            .as_ref()
            .into_iter()
            .flat_map(|page| {
                [page.assets.source, page.assets.rendered]
                    .into_iter()
                    .flatten()
                    .chain(page.elements.iter().filter_map(|element| element.image))
            })
            .filter_map(|blob| self.resources.color(blob))
            .collect::<Vec<_>>();
        for image in ready_images {
            self.gpu.mark_image_dirty(&image);
        }
        self.sync_ready_masks()?;
        self.damage.content();
        Ok(())
    }

    pub fn show_snapshot(&mut self, snapshot: &Snapshot, page: Option<PageId>) -> Result<()> {
        if let Some(page) = page {
            self.show_page(snapshot, page)
        } else {
            self.clear_page();
            self.snapshot = Some(snapshot.clone());
            self.revision = snapshot.revision();
            Ok(())
        }
    }

    pub fn clear_page(&mut self) {
        self.snapshot = None;
        self.page = None;
        self.revision = Revision::ZERO;
        self.masks.clear();
        self.element_scenes.clear();
        self.displayed_base = None;
        self.transition = None;
        self.reported_fallback = None;
        self.stroke = None;
        self.raster_stroke = None;
        self.transform = None;
        self.damage.content();
    }

    pub fn sync(&mut self, snapshot: &Snapshot, changes: &Change) -> Result<()> {
        let Some(current) = self.page.as_ref().map(|page| page.id) else {
            self.snapshot = Some(snapshot.clone());
            self.revision = snapshot.revision();
            return Ok(());
        };
        if changes.from != self.revision || changes.to != snapshot.revision() {
            return Err(Error::RevisionConflict {
                page: current,
                expected: self.revision,
                actual: if changes.from != self.revision {
                    changes.from
                } else {
                    snapshot.revision()
                },
            });
        }

        if changes.entities.contains(&EntityChange::Removed(current)) {
            self.clear_page();
            self.snapshot = Some(snapshot.clone());
            self.revision = changes.to;
            return Ok(());
        }

        let affected = !changes.entities.is_empty()
            || changes.hierarchy.iter().any(|entity| {
                *entity == current
                    || self
                        .page
                        .as_ref()
                        .is_some_and(|page| page.contains(*entity))
            })
            || !changes.relations.is_empty()
            || changes.components.iter().any(|change| match change.owner {
                ComponentOwner::Project | ComponentOwner::Relation(_) => true,
                ComponentOwner::Entity(entity) => {
                    self.page.as_ref().is_some_and(|page| page.contains(entity))
                }
            });
        let presentation_only = changes.entities.is_empty()
            && changes.hierarchy.is_empty()
            && changes.relations.is_empty()
            && !changes.components.is_empty()
            && changes
                .components
                .iter()
                .all(|change| change.kind == Visibility::KIND);
        let hierarchy_only = changes.entities.is_empty()
            && changes.relations.is_empty()
            && changes.components.is_empty()
            && !changes.hierarchy.is_empty();
        let typography_entities = (changes.entities.is_empty()
            && changes.hierarchy.is_empty()
            && changes.relations.is_empty()
            && !changes.components.is_empty()
            && changes
                .components
                .iter()
                .all(|change| change.kind == Typography::KIND))
        .then(|| {
            changes
                .components
                .iter()
                .filter_map(|change| match change.owner {
                    ComponentOwner::Entity(entity) => Some(entity),
                    ComponentOwner::Project | ComponentOwner::Relation(_) => None,
                })
                .collect::<HashSet<_>>()
        })
        .filter(|entities| !entities.is_empty());
        self.snapshot = Some(snapshot.clone());
        self.revision = changes.to;
        if !affected {
            return Ok(());
        }

        self.transform = None;
        let next = CanvasPage::load(snapshot, current)?;
        self.verify_mask_replacement(&next)?;
        let same_elements = hierarchy_only
            && self.page.as_ref().is_some_and(|page| {
                page.elements.len() == next.elements.len()
                    && page
                        .elements
                        .iter()
                        .map(|element| element.id)
                        .collect::<HashSet<_>>()
                        == next
                            .elements
                            .iter()
                            .map(|element| element.id)
                            .collect::<HashSet<_>>()
            });
        self.page = Some(next);
        if presentation_only || same_elements {
            self.element_scenes.recompose();
        } else if let Some(entities) = typography_entities {
            self.element_scenes.invalidate_text_entities(entities);
        } else {
            self.element_scenes.clear();
        }
        self.request_page_resources(snapshot);
        self.sync_ready_masks()?;
        self.damage.content();
        Ok(())
    }

    pub fn set_view(&mut self, view: crate::ViewState) {
        if !self.render_target_explicit && self.render_size != view.size {
            self.render_size = view.size;
            self.damage.target();
        }
        if self.view.camera != view.camera || self.view.display != view.display {
            self.damage.content();
        }
        if self.view.display.show_text != view.display.show_text {
            self.element_scenes.recompose();
        }
        if self.view.display.page != view.display.page {
            self.element_scenes.recompose();
            self.reported_fallback = None;
        }
        if let Some(transition) = self.transition.as_mut() {
            match view
                .display
                .transition
                .filter(|duration| !duration.is_zero())
            {
                Some(duration) => transition.duration = duration,
                None => {
                    self.displayed_base = Some(transition.to);
                    self.transition = None;
                }
            }
        }
        self.view = view;
    }

    #[must_use]
    pub const fn view(&self) -> &crate::ViewState {
        &self.view
    }

    /// Configures the full-window Vello target while keeping camera and pointer
    /// coordinates relative to the React canvas viewport.
    pub fn set_render_target(&mut self, size: PhysicalSize, origin: PhysicalPoint) {
        self.render_target_explicit = true;
        if self.render_size != size {
            self.render_size = size;
            self.damage.target();
        }
        if self.render_origin != origin && origin.x.is_finite() && origin.y.is_finite() {
            self.render_origin = origin;
            self.damage.content();
        }
    }

    pub fn set_text_options(&mut self, options: RenderTheme) {
        self.options.text = options;
        self.invalidate_text_scenes();
    }

    /// Call after the host installs or removes fonts used by the active project.
    pub fn invalidate_fonts(&mut self) {
        self.invalidate_text_scenes();
    }

    fn invalidate_text_scenes(&mut self) {
        self.element_scenes.invalidate_text();
        self.damage.content();
    }

    pub fn set_workspace_color(&mut self, color: [u8; 4]) {
        if self.options.workspace_color != color {
            self.options.workspace_color = color;
            self.damage.content();
        }
    }

    pub fn preview_opacity(&mut self, element: ElementId, opacity: Option<f32>) -> Result<()> {
        let opacity = match opacity {
            Some(opacity) if opacity.is_finite() && (0.0..=1.0).contains(&opacity) => opacity,
            Some(_) => {
                return Err(Error::Invalid(
                    "opacity preview must be between zero and one".into(),
                ));
            }
            None => self
                .snapshot
                .as_ref()
                .ok_or(Error::NoPage)?
                .component::<Visibility>(element)?
                .map_or(1.0, |visibility| visibility.opacity),
        };
        let element = self
            .page
            .as_mut()
            .ok_or(Error::NoPage)?
            .elements
            .iter_mut()
            .find(|candidate| candidate.id == element)
            .ok_or_else(|| {
                Error::Invalid("opacity preview element is not on the active page".into())
            })?;
        if element.opacity != opacity {
            element.opacity = opacity;
            self.element_scenes.recompose();
            self.damage.content();
        }
        Ok(())
    }

    #[must_use]
    pub fn screen_to_page(&self, point: PhysicalPoint) -> Option<PagePoint> {
        self.page.as_ref()?;
        if self.view.size.is_empty() || !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        Some(self.view.camera.screen_to_page(point))
    }

    #[must_use]
    pub fn page_to_screen(&self, point: PagePoint) -> PhysicalPoint {
        self.view.camera.page_to_screen(point)
    }

    #[must_use]
    pub fn page_id(&self) -> Option<PageId> {
        self.page.as_ref().map(|page| page.id)
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub(crate) fn page_size(&self) -> Option<PhysicalSize> {
        self.page.as_ref().map(|page| page.size)
    }

    fn contains_page_point(&self, point: PagePoint) -> bool {
        self.page.as_ref().is_some_and(|page| {
            point.x >= 0.0
                && point.y >= 0.0
                && point.x < f64::from(page.size.width)
                && point.y < f64::from(page.size.height)
        })
    }

    pub fn begin_raster_stroke(
        &mut self,
        layer: Option<ElementId>,
        brush: Brush,
        point: PagePoint,
    ) -> Result<()> {
        if self.raster_stroke.is_some() {
            return Err(Error::Invalid("a raster stroke is already active".into()));
        }
        if self.stroke.is_some() || self.transform.is_some() {
            return Err(Error::Invalid(
                "raster painting cannot start during another canvas edit".into(),
            ));
        }
        if !brush.diameter.is_finite() || !(1.0..=512.0).contains(&brush.diameter) {
            return Err(Error::Invalid(
                "brush diameter must be between 1 and 512 pixels".into(),
            ));
        }
        if !point.x.is_finite() || !point.y.is_finite() || !self.contains_page_point(point) {
            return Err(Error::Invalid(
                "raster stroke must begin inside the active page".into(),
            ));
        }
        if let Some(layer) = layer
            && self
                .page
                .as_ref()
                .and_then(|page| page.element(layer))
                .is_none_or(|element| element.raster.is_none())
        {
            return Err(Error::Invalid(
                "raster target is not an active pixel layer".into(),
            ));
        }
        self.raster_stroke = Some(RasterStrokeCommit {
            page: self.page_id().ok_or(Error::NoPage)?,
            layer,
            mode: brush.mode,
            color: brush.color,
            diameter: brush.diameter,
            points: vec![point],
        });
        self.damage.content();
        Ok(())
    }

    pub fn extend_raster_stroke(&mut self, points: &[PagePoint]) -> Result<()> {
        let size = self.page_size().ok_or(Error::NoPage)?;
        let zoom = self.view.camera.zoom().max(f64::EPSILON);
        let stroke = self.raster_stroke.as_mut().ok_or(Error::NoStroke)?;
        for point in points {
            if !point.x.is_finite() || !point.y.is_finite() {
                return Err(Error::Invalid("drawing points must be finite".into()));
            }
            let point = PagePoint::new(
                point.x.clamp(0.0, f64::from(size.width)),
                point.y.clamp(0.0, f64::from(size.height)),
            );
            if stroke
                .points
                .last()
                .is_none_or(|last| (last.x - point.x).hypot(last.y - point.y) >= 0.25 / zoom)
            {
                stroke.points.push(point);
            }
        }
        if !points.is_empty() {
            self.damage.content();
        }
        Ok(())
    }

    pub fn finish_raster_stroke(&mut self) -> Result<RasterStrokeCommit> {
        let stroke = self.raster_stroke.take().ok_or(Error::NoStroke)?;
        self.damage.content();
        Ok(stroke)
    }

    pub fn cancel_raster_stroke(&mut self) {
        if self.raster_stroke.take().is_some() {
            self.damage.content();
        }
    }

    pub fn begin_transform(&mut self, controls: &[ElementFrame]) -> Result<()> {
        if self.transform.is_some() {
            return Err(Error::Invalid(
                "an element transform is already active".into(),
            ));
        }
        if self.stroke.is_some() {
            return Err(Error::Invalid(
                "an element transform cannot start during a mask stroke".into(),
            ));
        }
        let page = self.page.as_ref().ok_or(Error::NoPage)?;
        self.transform = Some(ActiveTransform::new(page, controls)?);
        Ok(())
    }

    pub fn update_transform(&mut self, frame: u64, elements: &[ElementFrame]) -> Result<()> {
        let changed = self
            .transform
            .as_mut()
            .ok_or(Error::NoTransform)?
            .update(frame, elements)?;
        if changed {
            self.element_scenes.recompose();
            self.damage.content();
        }
        Ok(())
    }

    pub fn finish_transform(&mut self) -> Result<Option<TransformCommit>> {
        let transform = self.transform.take().ok_or(Error::NoTransform)?;
        self.element_scenes.recompose();
        self.damage.content();
        Ok(transform.finish())
    }

    pub fn cancel_transform(&mut self) {
        if self.transform.take().is_some() {
            self.element_scenes.recompose();
            self.damage.content();
        }
    }

    /// Returns renderer-resolved control frames for editable text.
    ///
    /// Other elements continue to use their scene geometry directly.
    pub fn element_frames(&mut self) -> Vec<ElementFrame> {
        let (Some(snapshot), Some(page)) = (self.snapshot.clone(), self.page.clone()) else {
            return Vec::new();
        };
        self.element_scenes.element_frames(
            &snapshot,
            &page,
            &self.options.text,
            &mut self.diagnostics,
        )
    }

    pub fn begin_mask_stroke(
        &mut self,
        plane: MaskPlane,
        brush: Brush,
        point: PagePoint,
    ) -> Result<()> {
        if self.stroke.is_some() {
            return Err(Error::Invalid("a mask stroke is already active".into()));
        }
        if !brush.diameter.is_finite() || brush.diameter <= 0.0 {
            return Err(Error::Invalid(
                "brush diameter must be finite and positive".into(),
            ));
        }
        let page = self.page.as_ref().ok_or(Error::NoPage)?;
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(Error::Invalid("stroke point must be finite".into()));
        }
        let mut page_point = point;
        page_point.x = page_point.x.clamp(0.0, f64::from(page.size.width));
        page_point.y = page_point.y.clamp(0.0, f64::from(page.size.height));
        let mut stroke = ActiveStroke::new(plane, brush, page_point);
        let state = self
            .masks
            .entry(plane)
            .or_insert_with(|| MaskState::empty(page.size));
        if page.assets.mask(plane).is_some() && state.source != page.assets.mask(plane) {
            return Err(Error::Invalid(format!(
                "{} mask is still loading",
                plane.name()
            )));
        }
        stroke.dirty = state.paint(page_point, page_point, brush, &mut stroke.before);
        self.stroke = Some(stroke);
        self.damage.content();
        Ok(())
    }

    pub fn extend_mask_stroke(&mut self, plane: MaskPlane, points: &[PagePoint]) -> Result<()> {
        let page = self.page.as_ref().ok_or(Error::NoPage)?;
        let stroke = self.stroke.as_mut().ok_or(Error::NoStroke)?;
        if stroke.plane != plane {
            return Err(Error::Invalid(format!(
                "active mask stroke is {}, not {}",
                stroke.plane.name(),
                plane.name()
            )));
        }
        let state = self.masks.get_mut(&stroke.plane).ok_or(Error::NoStroke)?;
        for point in points {
            if !point.x.is_finite() || !point.y.is_finite() {
                return Err(Error::Invalid("stroke points must be finite".into()));
            }
            let next = PagePoint::new(
                point.x.clamp(0.0, f64::from(page.size.width)),
                point.y.clamp(0.0, f64::from(page.size.height)),
            );
            let dirty = state.paint(stroke.last, next, stroke.brush, &mut stroke.before);
            stroke.dirty = stroke.dirty.union(dirty);
            stroke.last = next;
        }
        if !points.is_empty() {
            self.damage.content();
        }
        Ok(())
    }

    pub fn finish_mask_stroke(&mut self, plane: MaskPlane) -> Result<Option<MaskCommit>> {
        let page = self.page.as_ref().ok_or(Error::NoPage)?.id;
        if self
            .stroke
            .as_ref()
            .is_some_and(|stroke| stroke.plane != plane)
        {
            return Err(Error::Invalid(format!(
                "active mask stroke is not {}",
                plane.name()
            )));
        }
        let stroke = self.stroke.take().ok_or(Error::NoStroke)?;
        if stroke.dirty.is_empty() || stroke.before.is_empty() {
            return Ok(None);
        }
        let state = self.masks.get_mut(&stroke.plane).ok_or(Error::NoStroke)?;
        Ok(Some(state.finish(page, stroke.plane, stroke.dirty)))
    }

    pub fn cancel_mask_stroke(&mut self, plane: MaskPlane) -> Result<()> {
        if self
            .stroke
            .as_ref()
            .is_some_and(|stroke| stroke.plane != plane)
        {
            return Err(Error::Invalid(format!(
                "active mask stroke is not {}",
                plane.name()
            )));
        }
        if let Some(stroke) = self.stroke.take()
            && let Some(state) = self.masks.get_mut(&stroke.plane)
        {
            state.restore(stroke.before);
            self.damage.content();
        }
        Ok(())
    }

    pub fn clear_inpaint_mask(&mut self) {
        if let Some(size) = self.page_size() {
            self.masks
                .entry(MaskPlane::Inpaint)
                .or_insert_with(|| MaskState::empty(size))
                .replace(None, None, size);
            self.damage.content();
        }
    }

    pub fn acknowledge_mask_commit(
        &mut self,
        page: PageId,
        plane: MaskPlane,
        generation: u64,
        blob: BlobId,
    ) -> Result<()> {
        if self.page.as_ref().map(|page| page.id) != Some(page) {
            return Err(Error::Invalid(
                "mask commit belongs to a different page".into(),
            ));
        }
        self.masks
            .get_mut(&plane)
            .ok_or(Error::NoPage)?
            .acknowledge(generation, blob)
    }

    /// Produces the latest offscreen viewport texture.
    ///
    /// The stages are intentionally explicit: install newly decoded resources
    /// and rebuild Vello content only when required. `render` never presents a
    /// window surface; that is the desktop host's responsibility.
    pub fn render(&mut self, now: Instant) -> Result<CanvasFrame<'_>> {
        self.drain_resources()?;
        if self.damage.target_pending() {
            self.gpu.resize(self.render_size);
            self.damage.clear_target();
        }
        if self.render_size.is_empty() {
            return Ok(CanvasFrame {
                texture: self.gpu.output(),
                size: self.render_size,
                generation: self.generation,
                needs_redraw: false,
            });
        }

        self.update_transition(now);
        let needs_redraw = self.transition.is_some();

        if self.damage.content_pending() {
            let scene = self.build_scene(now, true);
            self.gpu
                .render_content(&scene, self.options.workspace_color)?;
            self.damage.clear_content();
            self.generation = self.generation.wrapping_add(1);
        }
        if needs_redraw {
            self.damage.content();
        }

        Ok(CanvasFrame {
            texture: self.gpu.output(),
            size: self.render_size,
            generation: self.generation,
            needs_redraw,
        })
    }

    pub fn take_diagnostics(&mut self) -> Vec<CanvasDiagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    pub fn sample_color(&mut self, point: PhysicalPoint) -> Result<[u8; 4]> {
        self.drain_resources()?;
        if self.damage.target_pending() {
            self.gpu.resize(self.render_size);
            self.damage.clear_target();
        }
        if self.render_size.is_empty() {
            return Err(Error::Invalid("cannot sample an empty canvas".into()));
        }
        self.update_transition(Instant::now());
        let scene = self.build_scene(Instant::now(), false);
        self.gpu
            .render_content(&scene, self.options.workspace_color)?;
        let x = self.render_origin.x + point.x;
        let y = self.render_origin.y + point.y;
        let color = self.gpu.read_pixel(x, y)?;
        self.damage.content();
        Ok(color)
    }

    #[cfg(test)]
    pub(crate) fn read_output_for_test(&self) -> Vec<u8> {
        self.gpu.read_output()
    }

    fn request_page_resources(&mut self, snapshot: &Snapshot) {
        let Some(page) = self.page.as_ref() else {
            return;
        };
        let id = page.id;
        // Blob reads are cheap database/storage operations here; image decoding
        // is delegated to Resources and completes asynchronously.
        let mut resources = vec![(
            page.assets
                .source
                .expect("CanvasPage requires a source asset"),
            ResourceKind::Color,
        )];
        resources.extend(
            [page.assets.rendered]
                .into_iter()
                .flatten()
                .map(|blob| (blob, ResourceKind::Color)),
        );
        resources.extend(
            page.assets
                .text_mask
                .into_iter()
                .map(|blob| (blob, ResourceKind::Gray)),
        );
        resources.extend(
            page.elements
                .iter()
                .filter_map(|element| element.image.map(|blob| (blob, ResourceKind::Color))),
        );
        resources.sort_unstable_by_key(|(blob, kind)| (*blob, *kind as u8));
        resources.dedup();
        for (blob, kind) in resources {
            if self.resources.contains(blob, kind) {
                continue;
            }
            match snapshot.read_blob(blob) {
                Ok(bytes) => self.resources.request(blob, kind, bytes),
                Err(error) => self.diagnostics.push(CanvasDiagnostic::resource(
                    Some(id),
                    blob,
                    error.to_string(),
                )),
            }
        }
    }

    fn active_blobs(&self) -> HashSet<BlobId> {
        let mut active = HashSet::new();
        let Some(page) = self.page.as_ref() else {
            return active;
        };
        active.insert(
            page.assets
                .source
                .expect("CanvasPage requires a source asset"),
        );
        active.extend(
            [page.assets.rendered, page.assets.text_mask]
                .into_iter()
                .flatten(),
        );
        active.extend(page.elements.iter().filter_map(|element| element.image));
        active
    }

    fn drain_resources(&mut self) -> Result<()> {
        // Worker results are installed only if their blob is still referenced
        // by the visible page. This prevents a late page-A decode from
        // invalidating page B after a page switch.
        let active = self.active_blobs();
        let events = self.resources.drain(&active);
        if events.is_empty() {
            return Ok(());
        }
        for event in events {
            match event {
                ResourceEvent::Ready { id, kind } => {
                    match kind {
                        ResourceKind::Gray => self.install_gray_resource(id)?,
                        ResourceKind::Color => {
                            self.element_scenes.invalidate_image(id);
                        }
                    }
                    self.damage.content();
                }
                ResourceEvent::Failed { id, kind, message } => {
                    self.diagnostics.push(CanvasDiagnostic::resource(
                        self.page.as_ref().map(|page| page.id),
                        id,
                        format!("failed to decode {kind:?} resource: {message}"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn install_gray_resource(&mut self, id: BlobId) -> Result<()> {
        let Some(page) = self.page.as_ref() else {
            return Ok(());
        };
        let image = self.resources.gray(id);
        let plane = MaskPlane::Text;
        if page.assets.mask(plane) != Some(id) {
            return Ok(());
        }
        let state = self
            .masks
            .entry(plane)
            .or_insert_with(|| MaskState::empty(page.size));
        if state.source == Some(id) {
            return Ok(());
        }
        if state.has_uncommitted() {
            return Err(Error::MaskConflict {
                page: page.id,
                plane: plane.name(),
            });
        }
        state.replace(Some(id), image.as_deref(), page.size);
        Ok(())
    }

    fn verify_mask_replacement(&self, next: &CanvasPage) -> Result<()> {
        let Some(current) = self.page.as_ref() else {
            return Ok(());
        };
        let plane = MaskPlane::Text;
        let before = current.assets.mask(plane);
        let after = next.assets.mask(plane);
        if before != after
            && self
                .masks
                .get(&plane)
                .is_some_and(MaskState::has_uncommitted)
            && self.masks.get(&plane).and_then(|state| state.source) != after
        {
            return Err(Error::MaskConflict {
                page: current.id,
                plane: plane.name(),
            });
        }
        if before != after
            && self
                .stroke
                .as_ref()
                .is_some_and(|stroke| stroke.plane == plane)
        {
            return Err(Error::MaskConflict {
                page: current.id,
                plane: plane.name(),
            });
        }
        Ok(())
    }

    fn sync_ready_masks(&mut self) -> Result<()> {
        let Some(page) = self.page.as_ref() else {
            return Ok(());
        };
        let size = page.size;
        let id = page.id;
        let plane = MaskPlane::Text;
        let desired = page.assets.mask(plane);
        let state = self
            .masks
            .entry(plane)
            .or_insert_with(|| MaskState::empty(size));
        if state.source == desired {
            return Ok(());
        }
        if state.has_uncommitted() {
            return Err(Error::MaskConflict {
                page: id,
                plane: plane.name(),
            });
        }
        match desired {
            None => state.replace(None, None, size),
            Some(blob) => {
                if let Some(image) = self.resources.gray(blob) {
                    state.replace(Some(blob), Some(&image), size);
                } else if state.source.is_some() {
                    state.replace(None, None, size);
                }
            }
        }
        Ok(())
    }

    fn build_scene(&mut self, now: Instant, include_previews: bool) -> Scene {
        // Everything in page_scene uses page coordinates. React and Rust share
        // a viewport-relative camera; render_origin moves that viewport into
        // the full desktop target without changing interaction coordinates.
        let mut scene = Scene::new();
        let Some(page) = self.page.clone() else {
            return scene;
        };
        let Some(snapshot) = self.snapshot.clone() else {
            return scene;
        };
        let mut page_scene = Scene::new();
        self.draw_base(&mut page_scene, &page, now);
        if self.view.display.page.is_editable() {
            if include_previews {
                self.draw_masks(&mut page_scene);
            }
            let elements = self.element_scenes.scene(ElementSceneContext {
                snapshot: &snapshot,
                page: &page,
                resources: &mut self.resources,
                text: &self.options.text,
                transform: self.transform.as_ref(),
                show_text: self.view.display.show_text,
                diagnostics: &mut self.diagnostics,
            });
            page_scene.append(elements, None);
            if include_previews
                && let Some(stroke) = self.raster_stroke.as_ref()
                && stroke.mode == StrokeMode::Paint
            {
                draw_freehand(
                    &mut page_scene,
                    &stroke.points,
                    stroke.diameter,
                    stroke.color,
                );
            }
        }
        let page_rect = Rect::new(
            0.0,
            0.0,
            f64::from(page.size.width),
            f64::from(page.size.height),
        );
        let viewport_rect = Rect::new(
            self.render_origin.x,
            self.render_origin.y,
            self.render_origin.x + f64::from(self.view.size.width),
            self.render_origin.y + f64::from(self.view.size.height),
        );
        let camera = Affine::translate(Vec2::new(self.render_origin.x, self.render_origin.y))
            * self.view.camera.affine();
        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &viewport_rect);
        scene.push_clip_layer(Fill::NonZero, camera, &page_rect);
        scene.append(&page_scene, Some(camera));
        scene.pop_layer();
        scene.pop_layer();
        scene
    }

    fn draw_base(&mut self, scene: &mut Scene, page: &CanvasPage, now: Instant) {
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            VelloColor::WHITE,
            None,
            &Rect::new(
                0.0,
                0.0,
                f64::from(page.size.width),
                f64::from(page.size.height),
            ),
        );
        if let Some(transition) = self.transition.as_ref() {
            let from = transition.from;
            let to = transition.to;
            let elapsed = now.saturating_duration_since(transition.started);
            let progress =
                (elapsed.as_secs_f32() / transition.duration.as_secs_f32()).clamp(0.0, 1.0);
            self.draw_page_image(scene, page, from, 1.0);
            self.draw_page_image(scene, page, to, progress);
        } else if let Some(blob) = self.displayed_base {
            self.draw_page_image(scene, page, blob, 1.0);
        }
    }

    fn draw_page_image(
        &mut self,
        scene: &mut Scene,
        page: &CanvasPage,
        blob: BlobId,
        opacity: f32,
    ) {
        let Some(image) = self.resources.color(blob) else {
            return;
        };
        if opacity < 1.0 {
            scene.push_layer(
                Fill::NonZero,
                Mix::Normal,
                opacity,
                Affine::IDENTITY,
                &Rect::new(
                    0.0,
                    0.0,
                    f64::from(page.size.width),
                    f64::from(page.size.height),
                ),
            );
        }
        scene.draw_image(&image, Affine::IDENTITY);
        if opacity < 1.0 {
            scene.pop_layer();
        }
    }

    fn draw_masks(&mut self, scene: &mut Scene) {
        let active = self.stroke.as_ref().map(|stroke| stroke.plane);
        let displays = [
            (
                MaskPlane::Text,
                self.view.display.text_mask.or_else(|| {
                    (active == Some(MaskPlane::Text))
                        .then_some(crate::MaskOverlay::new([244, 63, 94, 210], 0.55))
                }),
            ),
            (
                MaskPlane::Inpaint,
                (active == Some(MaskPlane::Inpaint))
                    .then_some(crate::MaskOverlay::new([168, 85, 247, 210], 0.55)),
            ),
        ];
        for (plane, overlay) in displays {
            let Some(overlay) = overlay else {
                continue;
            };
            let Some(mask) = self.masks.get_mut(&plane) else {
                continue;
            };
            for (x, y, image) in mask.tinted_tiles(overlay) {
                scene.draw_image(
                    &image,
                    Affine::translate(Vec2::new(f64::from(x), f64::from(y))),
                );
            }
        }
    }

    fn update_transition(&mut self, now: Instant) {
        if self.page.is_none() {
            self.displayed_base = None;
            self.transition = None;
            return;
        }
        if self.transition.as_ref().is_some_and(|transition| {
            now.saturating_duration_since(transition.started) >= transition.duration
        }) {
            self.displayed_base = self.transition.take().map(|transition| transition.to);
        }
        let target = self.resolved_base();
        let intended = self
            .transition
            .as_ref()
            .map_or(self.displayed_base, |transition| Some(transition.to));
        if intended == Some(target) {
            return;
        }
        let Some(from) = self.displayed_base else {
            self.displayed_base = Some(target);
            self.damage.content();
            return;
        };
        let duration = self
            .view
            .display
            .transition
            .filter(|duration| !duration.is_zero());
        if let Some(duration) = duration {
            self.transition = Some(ImageTransition {
                from,
                to: target,
                started: now,
                duration,
            });
        } else {
            self.displayed_base = Some(target);
            self.transition = None;
        }
        self.damage.content();
    }

    fn resolved_base(&mut self) -> BlobId {
        let page = self.page.as_ref().expect("resolved base requires a page");
        let (optional, view) = match self.view.display.page {
            PageView::Editable => (page.assets.source, None),
            PageView::Rendered => (page.assets.rendered, Some(PageView::Rendered)),
        };
        if let Some(blob) = optional
            && self.resources.contains(blob, ResourceKind::Color)
        {
            self.reported_fallback = None;
            return blob;
        }
        if let Some(view) = view {
            let key = (page.id, view);
            if self.reported_fallback != Some(key) && optional.is_none() {
                self.diagnostics.push(CanvasDiagnostic {
                    page: Some(page.id),
                    element: None,
                    blob: None,
                    message: format!("{view:?} image is unavailable; using the source"),
                });
                self.reported_fallback = Some(key);
            }
        }
        page.assets
            .source
            .expect("CanvasPage requires a source asset")
    }
}

fn draw_freehand(scene: &mut Scene, points: &[PagePoint], diameter: f32, color: [u8; 4]) {
    let Some(first) = points.first() else {
        return;
    };
    let color = VelloColor::from_rgba8(color[0], color[1], color[2], color[3]);
    if points.len() == 1 {
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            color,
            None,
            &Circle::new((first.x, first.y), f64::from(diameter) * 0.5),
        );
        return;
    }
    let mut path = BezPath::new();
    path.move_to((first.x, first.y));
    for point in &points[1..] {
        path.line_to((point.x, point.y));
    }
    scene.stroke(
        &Stroke::new(f64::from(diameter)),
        Affine::IDENTITY,
        color,
        None,
        &path,
    );
}
