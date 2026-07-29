use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use koharu_renderer::RenderTheme;
use koharu_scene::{BlobId, EntityChange, Revision, SceneChangeSet, SceneSnapshot};
use vello::{
    Scene,
    kurbo::{Affine, Rect, Vec2},
    peniko::{Color as VelloColor, Fill, Mix},
};

use crate::damage::RenderDamage;
use crate::{
    ActiveStroke, ActiveTransform, Brush, CanvasDiagnostic, CanvasElement, CanvasGpu,
    CanvasOptions, CanvasPage, ElementFrame, ElementId, ElementSceneContext, ElementScenes, Error,
    GpuRenderer, MaskCommit, MaskPlane, MaskState, PageId, PagePoint, PageView, PhysicalPoint,
    PhysicalSize, ResourceEvent, ResourceKind, Resources, Result, TransformCommit,
};

pub struct CanvasFrame<'a> {
    /// Final Vello pixels for the desktop surface. React draws all editor
    /// chrome in the transparent webview above this texture.
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
    render_size: PhysicalSize,
    render_origin: PhysicalPoint,
    render_target_explicit: bool,
    snapshot: Option<SceneSnapshot>,
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
            transform: None,
            diagnostics: Vec::new(),
            damage: RenderDamage::initial(),
            generation: 0,
        })
    }

    pub fn show_page(&mut self, snapshot: &SceneSnapshot, page: PageId) -> Result<()> {
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
        self.resources
            .request(source_id, ResourceKind::Color, source);
        self.request_page_resources(snapshot);
        self.sync_ready_masks()?;
        self.damage.content();
        Ok(())
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
        self.transform = None;
        self.damage.content();
    }

    pub fn sync(&mut self, snapshot: &SceneSnapshot, changes: &SceneChangeSet) -> Result<()> {
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

        let affected = changes.pages_changed
            || !changes.entities.is_empty()
            || !changes.relations.is_empty()
            || changes.components.iter().any(|change| {
                self.page
                    .as_ref()
                    .is_some_and(|page| page.contains(change.entity))
            });
        self.snapshot = Some(snapshot.clone());
        self.revision = changes.to;
        if !affected {
            return Ok(());
        }

        self.transform = None;
        let next = CanvasPage::load(snapshot, current)?;
        self.verify_mask_replacement(&next)?;
        self.page = Some(next);
        self.element_scenes
            .retain_page(self.page.as_ref().expect("active page was refreshed"));
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

    pub fn set_locale(&mut self, locale: Option<koharu_scene::LanguageTag>) {
        if self.options.locale != locale {
            self.options.locale = locale;
            self.invalidate_text_scenes();
        }
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

    pub fn begin_transform(&mut self, selected: &[ElementId]) -> Result<()> {
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
        let selectable = selected
            .iter()
            .copied()
            .filter(|element| {
                page.element(*element)
                    .is_some_and(CanvasElement::selectable)
            })
            .collect::<Vec<_>>();
        self.transform = Some(ActiveTransform::new(page, &selectable)?);
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
            let scene = self.build_scene(now);
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

    #[cfg(test)]
    pub(crate) fn read_output_for_test(&self) -> Vec<u8> {
        self.gpu.read_output()
    }

    fn request_page_resources(&mut self, snapshot: &SceneSnapshot) {
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
            [page.assets.clean, page.assets.rendered]
                .into_iter()
                .flatten()
                .map(|blob| (blob, ResourceKind::Color)),
        );
        resources.extend(
            [page.assets.text_mask, page.assets.brush_mask]
                .into_iter()
                .flatten()
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
            [
                page.assets.clean,
                page.assets.rendered,
                page.assets.text_mask,
                page.assets.brush_mask,
            ]
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
        for plane in [MaskPlane::Text, MaskPlane::Brush] {
            if page.assets.mask(plane) != Some(id) {
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

    fn verify_mask_replacement(&self, next: &CanvasPage) -> Result<()> {
        let Some(current) = self.page.as_ref() else {
            return Ok(());
        };
        for plane in [MaskPlane::Text, MaskPlane::Brush] {
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
            let desired = page.assets.mask(plane);
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
            self.draw_masks(&mut page_scene);
            let elements = self.element_scenes.scene(ElementSceneContext {
                snapshot: &snapshot,
                page: &page,
                resources: &mut self.resources,
                text: &self.options.text,
                locale: self.options.locale.as_ref(),
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
            PageView::EditableSource => (page.assets.source, None),
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
        page.assets
            .source
            .expect("CanvasPage requires a source asset")
    }
}
