use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use koharu_renderer::PageRenderOptions;
use koharu_scene::{
    BlobId, ChangeSet, Element, ElementId, Frame, Page, PageAsset, PageId, Revision, Session,
};
use vello::{
    Scene,
    kurbo::{Affine, Rect, Vec2},
    peniko::{Color as VelloColor, Fill, Mix},
};

use crate::damage::RenderDamage;
use crate::{
    ActiveStroke, ActiveTransform, Brush, Camera, CanvasDiagnostic, CanvasGpu, CanvasOptions,
    ElementSceneContext, ElementScenes, Error, GpuRenderer, Guide, Handle, HitTarget, MaskCommit,
    MaskPlane, MaskState, OverlayGeometry, OverlayState, PagePoint, PageView, PhysicalPoint,
    PhysicalSize, ResourceEvent, ResourceKind, Resources, Result, TransformCommit, frame_contains,
    frame_corners,
};

// Handles are editor controls, so they remain a fixed physical-pixel size
// instead of shrinking and growing with the page zoom. Their hit target is
// deliberately larger than the painted shape: this keeps the overlay tidy
// while making the controls forgiving to grab with a mouse or trackpad.
const HANDLE_VISUAL_SIZE: f64 = 16.0;
const HANDLE_HIT_SIZE: f64 = 28.0;
const ROTATE_HANDLE_OFFSET: f64 = 32.0;

pub struct CanvasFrame<'a> {
    /// Final page pixels plus editor chrome. The desktop host samples this view
    /// into its window surface; ownership remains with `Canvas`.
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

/// Rust-owned editor viewport. The desktop host only presents the returned texture.
pub struct Canvas {
    // GPU details are intentionally hidden behind one backend.
    gpu: GpuRenderer,
    options: CanvasOptions,

    // Authoritative presentation inputs and asynchronously decoded assets.
    resources: Resources,
    view: crate::ViewState,
    overlays: OverlayState,
    page: Option<Page>,
    revision: Revision,

    // Derived data that may be rebuilt without mutating the Session.
    masks: HashMap<MaskPlane, MaskState>,
    element_scenes: ElementScenes,
    displayed_base: Option<BlobId>,
    transition: Option<ImageTransition>,
    reported_fallback: Option<(PageId, PageView)>,

    // At most one low-latency editing operation is active for each category.
    stroke: Option<ActiveStroke>,
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
        let gpu = GpuRenderer::new(gpu, view.size)?;
        Ok(Self {
            gpu,
            options,
            resources,
            view,
            overlays: OverlayState::default(),
            page: None,
            revision: Revision::ZERO,
            masks: HashMap::new(),
            element_scenes: ElementScenes::new()?,
            displayed_base: None,
            transition: None,
            reported_fallback: None,
            stroke: None,
            transform: None,
            diagnostics: Vec::new(),
            damage: RenderDamage::initial(),
            generation: 0,
        })
    }

    pub fn show_page(&mut self, session: &Session, page: PageId) -> Result<()> {
        let next = session.page(page)?.clone();
        let source = session.read_blob(next.source)?;

        self.page = Some(next);
        self.revision = session.revision();
        self.stroke = None;
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
            MaskPlane::Brush,
            MaskState::empty(self.page.as_ref().expect("page was set").size),
        );
        self.resources.request(
            self.page.as_ref().expect("page was set").source,
            ResourceKind::Color,
            source,
        );
        self.request_page_resources(session);
        self.sync_ready_masks()?;
        self.damage.content();
        Ok(())
    }

    pub fn clear_page(&mut self) {
        self.page = None;
        self.revision = Revision::ZERO;
        self.masks.clear();
        self.element_scenes.clear();
        self.displayed_base = None;
        self.transition = None;
        self.reported_fallback = None;
        self.stroke = None;
        self.transform = None;
        self.damage.content();
    }

    pub fn sync(&mut self, session: &Session, changes: &ChangeSet) -> Result<()> {
        let Some(current) = self.page.as_ref().map(|page| page.id) else {
            self.revision = session.revision();
            return Ok(());
        };
        if changes.from != self.revision || changes.to != session.revision() {
            return Err(Error::RevisionConflict {
                page: current,
                expected: self.revision,
                actual: if changes.from != self.revision {
                    changes.from
                } else {
                    session.revision()
                },
            });
        }

        let affected = changes.pages.contains(&current)
            || changes.elements.iter().any(|id| {
                self.page
                    .as_ref()
                    .is_some_and(|page| page.element(*id).is_some())
            });
        self.revision = changes.to;
        if !affected {
            return Ok(());
        }

        self.transform = None;
        let next = session.page(current)?.clone();
        self.verify_mask_replacement(&next)?;
        self.page = Some(next);
        self.element_scenes
            .retain_page(self.page.as_ref().expect("active page was refreshed"));
        self.request_page_resources(session);
        self.sync_ready_masks()?;
        self.damage.content();
        Ok(())
    }

    pub fn set_view(&mut self, view: crate::ViewState) {
        if self.view.size != view.size {
            self.damage.target();
        }
        if self.view.camera != view.camera || self.view.display != view.display {
            self.damage.content();
        }
        if self.view.display.show_text != view.display.show_text {
            self.element_scenes.recompose();
        }
        if self.view.display.page != view.display.page {
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
        if self.view.camera != view.camera {
            self.damage.overlay();
        }
        self.view = view;
    }

    pub fn set_overlays(&mut self, overlays: OverlayState) {
        if self.overlays != overlays {
            self.overlays = overlays;
            self.damage.overlay();
        }
    }

    pub fn set_text_options(&mut self, options: PageRenderOptions) {
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
    pub fn hit_test(&self, point: PhysicalPoint) -> Option<HitTarget> {
        let page = self.page.as_ref()?;
        let page_point = self.screen_to_page(point)?;
        for &element in self.overlays.selected.iter().rev() {
            let Some(selected) = page.element(element) else {
                continue;
            };
            if !selected.visible || selected.opacity <= 0.0 {
                continue;
            }
            let frame = self.preview_frame(selected);
            for (handle, position) in handle_positions(frame, self.view.camera) {
                if handle_contains(handle, position, point) {
                    return Some(HitTarget::Handle { element, handle });
                }
            }
        }
        page.elements
            .iter()
            .rev()
            .find(|element| {
                element.visible
                    && element.opacity > 0.0
                    && frame_contains(self.preview_frame(element), page_point)
            })
            .map(|element| HitTarget::Element(element.id))
    }

    pub fn begin_transform(
        &mut self,
        selected: &[ElementId],
        target: HitTarget,
        point: PhysicalPoint,
    ) -> Result<()> {
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
        let page_point = self.screen_to_page(point).ok_or(Error::NoPage)?;
        let page = self.page.as_ref().ok_or(Error::NoPage)?;
        self.transform = Some(ActiveTransform::new(page, selected, target, page_point)?);
        self.damage.overlay();
        Ok(())
    }

    pub fn update_transform(&mut self, point: PhysicalPoint) -> Result<()> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(Error::Invalid(
                "transform point must contain finite coordinates".into(),
            ));
        }
        let page_point = self.view.camera.screen_to_page(point);
        self.transform
            .as_mut()
            .ok_or(Error::NoTransform)?
            .update(page_point)?;
        self.element_scenes.recompose();
        self.damage.content();
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

    pub fn begin_mask_stroke(
        &mut self,
        plane: MaskPlane,
        brush: Brush,
        point: PhysicalPoint,
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
        let mut page_point = self.screen_to_page(point).ok_or(Error::NoPage)?;
        page_point.x = page_point.x.clamp(0.0, f64::from(page.size.width));
        page_point.y = page_point.y.clamp(0.0, f64::from(page.size.height));
        let mut stroke = ActiveStroke::new(plane, brush, page_point);
        let state = self
            .masks
            .entry(plane)
            .or_insert_with(|| MaskState::empty(page.size));
        if page.assets.get(plane.asset()).is_some()
            && state.source != page.assets.get(plane.asset())
        {
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

    pub fn extend_mask_stroke(&mut self, point: PhysicalPoint) -> Result<()> {
        let page = self.page.as_ref().ok_or(Error::NoPage)?;
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(Error::Invalid("stroke point must be finite".into()));
        }
        let mut next = self.view.camera.screen_to_page(point);
        next.x = next.x.clamp(0.0, f64::from(page.size.width));
        next.y = next.y.clamp(0.0, f64::from(page.size.height));
        let stroke = self.stroke.as_mut().ok_or(Error::NoStroke)?;
        let state = self.masks.get_mut(&stroke.plane).ok_or(Error::NoStroke)?;
        let dirty = state.paint(stroke.last, next, stroke.brush, &mut stroke.before);
        stroke.dirty = stroke.dirty.union(dirty);
        stroke.last = next;
        self.damage.content();
        Ok(())
    }

    pub fn finish_mask_stroke(&mut self) -> Result<Option<MaskCommit>> {
        let page = self.page.as_ref().ok_or(Error::NoPage)?.id;
        let stroke = self.stroke.take().ok_or(Error::NoStroke)?;
        if stroke.dirty.is_empty() || stroke.before.is_empty() {
            return Ok(None);
        }
        let state = self.masks.get_mut(&stroke.plane).ok_or(Error::NoStroke)?;
        Ok(Some(state.finish(page, stroke.plane, stroke.dirty)))
    }

    pub fn cancel_mask_stroke(&mut self) {
        if let Some(stroke) = self.stroke.take()
            && let Some(state) = self.masks.get_mut(&stroke.plane)
        {
            state.restore(stroke.before);
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
    /// The stages are intentionally explicit: install newly decoded resources,
    /// rebuild Vello content only when required, then copy that stable content
    /// and draw inexpensive editor chrome. `render` never presents a window
    /// surface; that is the desktop host's responsibility.
    pub fn render(&mut self, now: Instant) -> Result<CanvasFrame<'_>> {
        self.drain_resources()?;
        if self.damage.target_pending() {
            self.gpu.resize(self.view.size);
            self.damage.clear_target();
        }
        if self.view.size.is_empty() {
            return Ok(CanvasFrame {
                texture: self.gpu.output(),
                size: self.view.size,
                generation: self.generation,
                needs_redraw: false,
            });
        }

        self.update_transition(now);
        let needs_redraw = self.transition.is_some();

        if self.damage.content_pending() {
            let scene = self.build_scene(now);
            self.gpu
                .render_content(&scene, self.options.workspace_color)?;
            self.damage.clear_content();
        }

        if self.damage.overlay_pending() {
            let geometry = self.build_overlay_geometry();
            self.gpu.compose_overlay(&geometry);
            self.damage.clear_overlay();
            self.generation = self.generation.wrapping_add(1);
        }
        if needs_redraw {
            self.damage.content();
        }

        Ok(CanvasFrame {
            texture: self.gpu.output(),
            size: self.view.size,
            generation: self.generation,
            needs_redraw,
        })
    }

    pub fn take_diagnostics(&mut self) -> Vec<CanvasDiagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    #[cfg(test)]
    pub(crate) fn read_output_for_test(&self) -> Vec<u8> {
        self.gpu.read_output()
    }

    fn request_page_resources(&mut self, session: &Session) {
        let Some(page) = self.page.as_ref() else {
            return;
        };
        let id = page.id;
        // Blob reads are cheap database/storage operations here; image decoding
        // is delegated to Resources and completes asynchronously.
        let mut resources = vec![(page.source, ResourceKind::Color)];
        resources.extend(
            [PageAsset::Clean, PageAsset::Rendered]
                .into_iter()
                .filter_map(|asset| page.assets.get(asset))
                .map(|blob| (blob, ResourceKind::Color)),
        );
        resources.extend(
            [PageAsset::TextMask, PageAsset::BrushMask]
                .into_iter()
                .filter_map(|asset| page.assets.get(asset))
                .map(|blob| (blob, ResourceKind::Gray)),
        );
        resources.extend(page.elements.iter().filter_map(|element| {
            element
                .image_data()
                .map(|image| (image.blob, ResourceKind::Color))
        }));
        resources.sort_unstable_by_key(|(blob, kind)| (*blob, *kind as u8));
        resources.dedup();
        for (blob, kind) in resources {
            if self.resources.contains(blob, kind) {
                continue;
            }
            match session.read_blob(blob) {
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
        active.insert(page.source);
        active.extend(
            [
                page.assets.clean,
                page.assets.rendered,
                page.assets.text_mask,
                page.assets.brush_mask,
            ]
            .into_iter()
            .flatten(),
        );
        active.extend(
            page.elements
                .iter()
                .filter_map(|element| element.image_data().map(|image| image.blob)),
        );
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
        for plane in [MaskPlane::Text, MaskPlane::Brush] {
            if page.assets.get(plane.asset()) != Some(id) {
                continue;
            }
            let state = self
                .masks
                .entry(plane)
                .or_insert_with(|| MaskState::empty(page.size));
            if state.source == Some(id) {
                continue;
            }
            if state.has_uncommitted() {
                return Err(Error::MaskConflict {
                    page: page.id,
                    plane: plane.name(),
                });
            }
            state.replace(Some(id), image.as_deref(), page.size);
        }
        Ok(())
    }

    fn verify_mask_replacement(&self, next: &Page) -> Result<()> {
        let Some(current) = self.page.as_ref() else {
            return Ok(());
        };
        for plane in [MaskPlane::Text, MaskPlane::Brush] {
            let before = current.assets.get(plane.asset());
            let after = next.assets.get(plane.asset());
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
        }
        Ok(())
    }

    fn sync_ready_masks(&mut self) -> Result<()> {
        let Some(page) = self.page.as_ref() else {
            return Ok(());
        };
        let size = page.size;
        let id = page.id;
        for plane in [MaskPlane::Text, MaskPlane::Brush] {
            let desired = page.assets.get(plane.asset());
            let state = self
                .masks
                .entry(plane)
                .or_insert_with(|| MaskState::empty(size));
            if state.source == desired {
                continue;
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
        }
        Ok(())
    }

    fn build_scene(&mut self, now: Instant) -> Scene {
        // Everything in page_scene uses page coordinates. One camera affine is
        // applied at the end, keeping element, mask, and hit-test math aligned.
        let mut scene = Scene::new();
        let Some(page) = self.page.clone() else {
            return scene;
        };
        let mut page_scene = Scene::new();
        self.draw_base(&mut page_scene, &page, now);
        if self.view.display.page.is_editable() {
            self.draw_masks(&mut page_scene);
            let elements = self.element_scenes.scene(ElementSceneContext {
                page: &page,
                resources: &mut self.resources,
                text: &self.options.text,
                transform: self.transform.as_ref(),
                show_text: self.view.display.show_text,
                diagnostics: &mut self.diagnostics,
            });
            page_scene.append(elements, None);
        }
        let page_rect = Rect::new(
            0.0,
            0.0,
            f64::from(page.size.width),
            f64::from(page.size.height),
        );
        scene.push_clip_layer(Fill::NonZero, self.view.camera.affine(), &page_rect);
        scene.append(&page_scene, Some(self.view.camera.affine()));
        scene.pop_layer();
        scene
    }

    fn draw_base(&mut self, scene: &mut Scene, page: &Page, now: Instant) {
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

    fn draw_page_image(&mut self, scene: &mut Scene, page: &Page, blob: BlobId, opacity: f32) {
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
        let displays = [
            (MaskPlane::Text, self.view.display.text_mask),
            (MaskPlane::Brush, self.view.display.brush_mask),
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
            PageView::EditableSource => (Some(page.source), None),
            PageView::EditableClean => (page.assets.clean, Some(PageView::EditableClean)),
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
        page.source
    }

    fn build_overlay_geometry(&self) -> OverlayGeometry {
        // OverlayGeometry uses physical pixels because handles and cursor
        // borders must remain readable at every camera zoom.
        let mut geometry = OverlayGeometry::default();
        let Some(page) = self.page.as_ref() else {
            return geometry;
        };
        let camera = self.view.camera;
        let page_width = f64::from(page.size.width);
        let page_height = f64::from(page.size.height);
        for guide in &self.overlays.guides {
            match *guide {
                Guide::Horizontal(y) => geometry.line(
                    camera.page_to_screen(PagePoint::new(0.0, y)),
                    camera.page_to_screen(PagePoint::new(page_width, y)),
                    1.0,
                    [80, 210, 255, 220],
                ),
                Guide::Vertical(x) => geometry.line(
                    camera.page_to_screen(PagePoint::new(x, 0.0)),
                    camera.page_to_screen(PagePoint::new(x, page_height)),
                    1.0,
                    [80, 210, 255, 220],
                ),
            }
        }
        if self.overlays.show_text_bounds {
            for (index, element) in page
                .elements
                .iter()
                .filter(|element| {
                    element.text().is_some() && element.visible && element.opacity > 0.0
                })
                .enumerate()
            {
                let corners = screen_corners(self.preview_frame(element), camera);
                geometry.outline(corners, 1.0, [255, 91, 145, 210]);
                geometry.label(corners[0], index + 1, [255, 91, 145, 240]);
            }
        }
        if let Some(frame) = self.overlays.draft {
            let corners = screen_corners(frame, camera);
            for index in 0..4 {
                geometry.dashed_line(
                    corners[index],
                    corners[(index + 1) % 4],
                    1.5,
                    6.0,
                    4.0,
                    [110, 170, 255, 255],
                );
            }
        }
        if let Some(element) = self.overlays.hovered
            && let Some(frame) = self.element_frame(element)
        {
            geometry.outline(screen_corners(frame, camera), 1.5, [80, 225, 235, 255]);
        }
        for &element in &self.overlays.selected {
            let Some(frame) = self.element_frame(element) else {
                continue;
            };
            geometry.outline(screen_corners(frame, camera), 2.0, [80, 145, 255, 255]);
            let positions = handle_positions(frame, camera);
            let north = positions[1].1;
            let rotate = positions[8].1;
            geometry.line(north, rotate, 1.5, [80, 145, 255, 255]);
            for (handle, point) in positions {
                if handle == Handle::Rotate {
                    geometry.circle_ring(point, HANDLE_VISUAL_SIZE * 0.5, 4.0, [80, 145, 255, 255]);
                } else {
                    geometry.solid_rect(
                        point,
                        HANDLE_VISUAL_SIZE,
                        HANDLE_VISUAL_SIZE,
                        [245, 248, 255, 255],
                    );
                    geometry.solid_rect(
                        point,
                        HANDLE_VISUAL_SIZE - 4.0,
                        HANDLE_VISUAL_SIZE - 4.0,
                        [80, 145, 255, 255],
                    );
                }
            }
        }
        if let Some(cursor) = self.overlays.brush_cursor {
            geometry.circle_ring(
                cursor.point,
                f64::from(cursor.diameter) * camera.zoom() * 0.5,
                1.5,
                [255, 255, 255, 230],
            );
        }
        geometry
    }

    fn preview_frame(&self, element: &Element) -> Frame {
        self.transform
            .as_ref()
            .and_then(|transform| transform.preview(element.id))
            .unwrap_or(element.frame)
    }

    fn element_frame(&self, id: ElementId) -> Option<Frame> {
        let element = self.page.as_ref()?.element(id)?;
        if !element.visible || element.opacity <= 0.0 {
            return None;
        }
        let frame = self.preview_frame(element);
        valid_frame(frame).then_some(frame)
    }
}

fn screen_corners(frame: Frame, camera: Camera) -> [PhysicalPoint; 4] {
    frame_corners(frame).map(|point| camera.page_to_screen(point))
}

fn handle_positions(frame: Frame, camera: Camera) -> [(Handle, PhysicalPoint); 9] {
    let [north_west, north_east, south_east, south_west] = screen_corners(frame, camera);
    let midpoint = |a: PhysicalPoint, b: PhysicalPoint| {
        PhysicalPoint::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
    };
    let north = midpoint(north_west, north_east);
    let angle = f64::from(frame.angle_degrees).to_radians();
    let rotate = PhysicalPoint::new(
        north.x + angle.sin() * ROTATE_HANDLE_OFFSET,
        north.y - angle.cos() * ROTATE_HANDLE_OFFSET,
    );
    [
        (Handle::NorthWest, north_west),
        (Handle::North, north),
        (Handle::NorthEast, north_east),
        (Handle::East, midpoint(north_east, south_east)),
        (Handle::SouthEast, south_east),
        (Handle::South, midpoint(south_east, south_west)),
        (Handle::SouthWest, south_west),
        (Handle::West, midpoint(south_west, north_west)),
        (Handle::Rotate, rotate),
    ]
}

fn handle_contains(handle: Handle, center: PhysicalPoint, point: PhysicalPoint) -> bool {
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    if handle == Handle::Rotate {
        dx.hypot(dy) <= HANDLE_HIT_SIZE * 0.5
    } else {
        dx.abs() <= HANDLE_HIT_SIZE * 0.5 && dy.abs() <= HANDLE_HIT_SIZE * 0.5
    }
}

fn valid_frame(frame: Frame) -> bool {
    frame.x.is_finite()
        && frame.y.is_finite()
        && frame.width.is_finite()
        && frame.width > 0.0
        && frame.height.is_finite()
        && frame.height > 0.0
        && frame.angle_degrees.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_handle_keeps_a_constant_screen_offset() {
        let frame = Frame::new(10.0, 20.0, 100.0, 50.0);
        for zoom in [0.25, 1.0, 4.0] {
            let camera = Camera::new(zoom, [12.0, 8.0]).unwrap();
            let positions = handle_positions(frame, camera);
            assert_eq!(positions[0].0, Handle::NorthWest);
            assert_eq!(positions[4].0, Handle::SouthEast);
            assert_eq!(positions[8].0, Handle::Rotate);
            let north = positions[1].1;
            let rotate = positions[8].1;
            assert!((north.x - rotate.x).abs() < 1e-9);
            assert!((north.y - rotate.y - ROTATE_HANDLE_OFFSET).abs() < 1e-9);
        }
        assert_eq!(HANDLE_VISUAL_SIZE, 16.0);
    }

    #[test]
    fn handles_have_a_larger_forgiving_hit_target() {
        let center = PhysicalPoint::new(100.0, 100.0);
        let just_inside = HANDLE_HIT_SIZE * 0.5 - 0.01;
        let just_outside = HANDLE_HIT_SIZE * 0.5 + 0.01;

        assert!(handle_contains(
            Handle::SouthEast,
            center,
            PhysicalPoint::new(center.x + just_inside, center.y + just_inside),
        ));
        assert!(!handle_contains(
            Handle::SouthEast,
            center,
            PhysicalPoint::new(center.x + just_outside, center.y),
        ));
        assert!(handle_contains(
            Handle::Rotate,
            center,
            PhysicalPoint::new(center.x, center.y + just_inside),
        ));
        assert!(!handle_contains(
            Handle::Rotate,
            center,
            PhysicalPoint::new(center.x, center.y + just_outside),
        ));
    }
}
