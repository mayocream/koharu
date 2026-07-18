use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use koharu_renderer::{BubbleIndex, PageRenderOptions, SceneRenderer};
use koharu_scene::{
    BlobId, ChangeSet, Element, ElementId, ElementKind, Frame, Page, PageAsset, PageId, Revision,
    Session,
};
use vello::{
    AaConfig, AaSupport, RenderParams, RendererOptions, Scene,
    kurbo::{Affine, Rect, Vec2},
    peniko::{Color as VelloColor, Fill, Mix},
};

use crate::{
    ActiveStroke, Brush, Camera, CanvasDiagnostic, CanvasGpu, CanvasOptions, Error, Guide, Handle,
    HitTarget, MaskCommit, MaskPlane, MaskState, OverlayGeometry, OverlayRenderer, OverlayState,
    PagePoint, PageView, PhysicalPoint, PhysicalSize, ResourceEvent, ResourceKind, Resources,
    Result, frame_contains, frame_corners,
};

const HANDLE_SIZE: f64 = 8.0;

pub struct CanvasFrame<'a> {
    pub texture: &'a wgpu::TextureView,
    pub size: PhysicalSize,
    pub generation: u64,
    pub needs_redraw: bool,
}

struct Targets {
    size: PhysicalSize,
    content: wgpu::Texture,
    content_view: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
}

struct CachedElement {
    element: Element,
    bubble_mask: Option<BlobId>,
    scene: Scene,
}

struct ImageTransition {
    from: BlobId,
    to: BlobId,
    started: Instant,
    duration: Duration,
}

impl Targets {
    fn new(device: &wgpu::Device, requested: PhysicalSize) -> Self {
        let size = PhysicalSize::new(requested.width.max(1), requested.height.max(1));
        let extent = wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };
        let content = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("koharu canvas content"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("koharu canvas output"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let content_view = content.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            size,
            content,
            content_view,
            output,
            output_view,
        }
    }
}

/// Rust-owned editor viewport. The desktop host only presents the returned texture.
pub struct Canvas {
    gpu: CanvasGpu,
    options: CanvasOptions,
    renderer: vello::Renderer,
    scene_renderer: SceneRenderer,
    overlay_renderer: OverlayRenderer,
    resources: Resources,
    targets: Targets,
    view: crate::ViewState,
    overlays: OverlayState,
    page: Option<Page>,
    revision: Revision,
    masks: HashMap<MaskPlane, MaskState>,
    bubble_index: Option<(BlobId, Arc<BubbleIndex>)>,
    element_cache: HashMap<ElementId, CachedElement>,
    elements_scene: Option<Scene>,
    elements_dirty: bool,
    displayed_base: Option<BlobId>,
    transition: Option<ImageTransition>,
    reported_fallback: Option<(PageId, PageView)>,
    stroke: Option<ActiveStroke>,
    diagnostics: Vec<CanvasDiagnostic>,
    content_dirty: bool,
    overlay_dirty: bool,
    target_dirty: bool,
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
        let renderer = vello::Renderer::new(
            &gpu.device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                ..Default::default()
            },
        )
        .map_err(|error| Error::Gpu(error.to_string()))?;
        let scene_renderer =
            SceneRenderer::new().map_err(|error| Error::Invalid(error.to_string()))?;
        let overlay_renderer = OverlayRenderer::new(&gpu.device);
        let resources = Resources::new(options.max_decoded_bytes, wake);
        let view = crate::ViewState::default();
        let targets = Targets::new(&gpu.device, view.size);
        Ok(Self {
            gpu,
            options,
            renderer,
            scene_renderer,
            overlay_renderer,
            resources,
            targets,
            view,
            overlays: OverlayState::default(),
            page: None,
            revision: Revision::ZERO,
            masks: HashMap::new(),
            bubble_index: None,
            element_cache: HashMap::new(),
            elements_scene: None,
            elements_dirty: true,
            displayed_base: None,
            transition: None,
            reported_fallback: None,
            stroke: None,
            diagnostics: Vec::new(),
            content_dirty: true,
            overlay_dirty: true,
            target_dirty: false,
            generation: 0,
        })
    }

    pub fn show_page(&mut self, session: &Session, page: PageId) -> Result<()> {
        let next = session.page(page)?.clone();
        let source = session.read_blob(next.source)?;

        self.page = Some(next);
        self.revision = session.revision();
        self.stroke = None;
        self.masks.clear();
        self.bubble_index = None;
        self.element_cache.clear();
        self.elements_scene = None;
        self.elements_dirty = true;
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
        self.content_dirty = true;
        self.overlay_dirty = true;
        Ok(())
    }

    pub fn clear_page(&mut self) {
        self.page = None;
        self.revision = Revision::ZERO;
        self.masks.clear();
        self.bubble_index = None;
        self.element_cache.clear();
        self.elements_scene = None;
        self.elements_dirty = true;
        self.displayed_base = None;
        self.transition = None;
        self.reported_fallback = None;
        self.stroke = None;
        self.content_dirty = true;
        self.overlay_dirty = true;
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

        let next = session.page(current)?.clone();
        self.verify_mask_replacement(&next)?;
        self.page = Some(next);
        if self.bubble_index.as_ref().map(|(blob, _)| *blob)
            != self.page.as_ref().and_then(|page| page.assets.bubble_mask)
        {
            self.bubble_index = None;
        }
        self.elements_dirty = true;
        self.elements_scene = None;
        self.element_cache.retain(|element, _| {
            self.page
                .as_ref()
                .is_some_and(|page| page.element(*element).is_some())
        });
        self.request_page_resources(session);
        self.sync_ready_masks()?;
        self.content_dirty = true;
        self.overlay_dirty = true;
        Ok(())
    }

    pub fn set_view(&mut self, view: crate::ViewState) {
        if self.view.size != view.size {
            self.target_dirty = true;
        }
        if self.view.camera != view.camera || self.view.display != view.display {
            self.content_dirty = true;
        }
        if self.view.display.show_text != view.display.show_text {
            self.elements_dirty = true;
            self.elements_scene = None;
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
            self.overlay_dirty = true;
        }
        self.view = view;
    }

    pub fn set_overlays(&mut self, overlays: OverlayState) {
        if self.overlays != overlays {
            self.overlays = overlays;
            self.overlay_dirty = true;
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
        self.element_cache
            .retain(|_, cached| cached.element.text().is_none());
        self.elements_scene = None;
        self.elements_dirty = true;
        self.content_dirty = true;
    }

    pub fn set_workspace_color(&mut self, color: [u8; 4]) {
        if self.options.workspace_color != color {
            self.options.workspace_color = color;
            self.content_dirty = true;
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
                if (point.x - position.x).abs() <= HANDLE_SIZE * 0.5
                    && (point.y - position.y).abs() <= HANDLE_SIZE * 0.5
                {
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
        self.content_dirty = true;
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
        self.content_dirty = true;
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
            self.content_dirty = true;
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

    pub fn render(&mut self, now: Instant) -> Result<CanvasFrame<'_>> {
        self.drain_resources()?;
        if self.target_dirty {
            self.targets = Targets::new(&self.gpu.device, self.view.size);
            self.target_dirty = false;
            self.content_dirty = true;
            self.overlay_dirty = true;
        }
        if self.view.size.is_empty() {
            return Ok(CanvasFrame {
                texture: &self.targets.output_view,
                size: self.view.size,
                generation: self.generation,
                needs_redraw: false,
            });
        }

        self.update_transition(now);
        let needs_redraw = self.transition.is_some();

        if self.content_dirty {
            let scene = self.build_scene(now);
            self.renderer
                .render_to_texture(
                    &self.gpu.device,
                    &self.gpu.queue,
                    &scene,
                    &self.targets.content_view,
                    &RenderParams {
                        base_color: vello_color(self.options.workspace_color),
                        width: self.targets.size.width,
                        height: self.targets.size.height,
                        antialiasing_method: AaConfig::Area,
                    },
                )
                .map_err(|error| Error::Gpu(error.to_string()))?;
            self.content_dirty = false;
            self.overlay_dirty = true;
        }

        if self.overlay_dirty {
            let geometry = self.build_overlay_geometry();
            let mut encoder =
                self.gpu
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("koharu canvas frame"),
                    });
            encoder.copy_texture_to_texture(
                self.targets.content.as_image_copy(),
                self.targets.output.as_image_copy(),
                wgpu::Extent3d {
                    width: self.targets.size.width,
                    height: self.targets.size.height,
                    depth_or_array_layers: 1,
                },
            );
            self.overlay_renderer.draw(
                &self.gpu.device,
                &self.gpu.queue,
                &mut encoder,
                &self.targets.output_view,
                self.targets.size,
                &geometry,
            );
            self.gpu.queue.submit([encoder.finish()]);
            self.overlay_dirty = false;
            self.generation = self.generation.wrapping_add(1);
        }
        if needs_redraw {
            self.content_dirty = true;
        }

        Ok(CanvasFrame {
            texture: &self.targets.output_view,
            size: self.view.size,
            generation: self.generation,
            needs_redraw,
        })
    }

    pub fn take_diagnostics(&mut self) -> Vec<CanvasDiagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    fn request_page_resources(&mut self, session: &Session) {
        let Some(page) = self.page.as_ref() else {
            return;
        };
        let id = page.id;
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
        resources.extend(
            page.assets
                .bubble_mask
                .map(|blob| (blob, ResourceKind::Bubble)),
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
                page.assets.bubble_mask,
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
                        ResourceKind::Bubble => {
                            if self
                                .page
                                .as_ref()
                                .is_some_and(|page| page.assets.bubble_mask == Some(id))
                                && let Some(index) = self.resources.bubble(id)
                            {
                                self.bubble_index = Some((id, index));
                                self.elements_dirty = true;
                                self.elements_scene = None;
                            }
                        }
                        ResourceKind::Color => {
                            self.element_cache.retain(|_, cached| {
                                cached.element.image_data().map(|image| image.blob) != Some(id)
                            });
                            self.elements_dirty = true;
                            self.elements_scene = None;
                        }
                    }
                    self.content_dirty = true;
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
        match page.assets.bubble_mask {
            Some(blob)
                if self.bubble_index.as_ref().map(|(id, _)| *id) != Some(blob)
                    && self.resources.contains(blob, ResourceKind::Bubble) =>
            {
                self.bubble_index = self.resources.bubble(blob).map(|index| (blob, index));
            }
            None => self.bubble_index = None,
            Some(_) => {}
        }
        Ok(())
    }

    fn build_scene(&mut self, now: Instant) -> Scene {
        let mut scene = Scene::new();
        let Some(page) = self.page.clone() else {
            return scene;
        };
        let mut page_scene = Scene::new();
        self.draw_base(&mut page_scene, &page, now);
        if self.view.display.page.is_editable() {
            self.draw_masks(&mut page_scene);
            self.ensure_elements_scene(&page);
            if let Some(elements) = self.elements_scene.as_ref() {
                page_scene.append(elements, None);
            }
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

    fn ensure_elements_scene(&mut self, page: &Page) {
        if !self.elements_dirty && self.elements_scene.is_some() {
            return;
        }
        let bubble_mask = self.bubble_index.as_ref().map(|(blob, _)| *blob);
        for element in &page.elements {
            if !element.visible || element.opacity <= 0.0 {
                continue;
            }
            if element.text().is_some() && !self.view.display.show_text {
                continue;
            }
            let expected_bubble = element.text().map(|_| bubble_mask).unwrap_or(None);
            let reusable = self.element_cache.get(&element.id).is_some_and(|cached| {
                cached.element == *element && cached.bubble_mask == expected_bubble
            });
            if reusable {
                continue;
            }
            let mut encoded = Scene::new();
            match &element.kind {
                ElementKind::Text(_) if self.view.display.show_text => {
                    match self.scene_renderer.encode_text_element(
                        &mut encoded,
                        element,
                        self.bubble_index.as_ref().map(|(_, index)| index.as_ref()),
                        &self.options.text,
                    ) {
                        Ok(Some(_)) | Ok(None) => {}
                        Err(error) => self.diagnostics.push(CanvasDiagnostic::element(
                            page.id,
                            element.id,
                            error.to_string(),
                        )),
                    }
                }
                ElementKind::Image(image) => {
                    let Some(data) = self.resources.color(image.blob) else {
                        continue;
                    };
                    SceneRenderer::encode_image_element(&mut encoded, element, &data);
                }
                ElementKind::Text(_) => {}
            }
            self.element_cache.insert(
                element.id,
                CachedElement {
                    element: element.clone(),
                    bubble_mask: expected_bubble,
                    scene: encoded,
                },
            );
        }
        self.element_cache
            .retain(|id, _| page.element(*id).is_some());
        let mut combined = Scene::new();
        for element in &page.elements {
            if !element.visible
                || element.opacity <= 0.0
                || (element.text().is_some() && !self.view.display.show_text)
            {
                continue;
            }
            if let Some(cached) = self.element_cache.get(&element.id) {
                combined.append(&cached.scene, None);
            }
        }
        self.elements_scene = Some(combined);
        self.elements_dirty = false;
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
            self.content_dirty = true;
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
        self.content_dirty = true;
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
            for (_, point) in handle_positions(frame, camera) {
                geometry.solid_rect(point, HANDLE_SIZE, HANDLE_SIZE, [245, 248, 255, 255]);
                geometry.solid_rect(
                    point,
                    HANDLE_SIZE - 3.0,
                    HANDLE_SIZE - 3.0,
                    [80, 145, 255, 255],
                );
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
        self.overlays
            .element_previews
            .iter()
            .find(|preview| preview.element == element.id)
            .map_or(element.frame, |preview| preview.frame)
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

fn handle_positions(frame: Frame, camera: Camera) -> [(Handle, PhysicalPoint); 8] {
    let [north_west, north_east, south_east, south_west] = screen_corners(frame, camera);
    let midpoint = |a: PhysicalPoint, b: PhysicalPoint| {
        PhysicalPoint::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
    };
    [
        (Handle::NorthWest, north_west),
        (Handle::North, midpoint(north_west, north_east)),
        (Handle::NorthEast, north_east),
        (Handle::East, midpoint(north_east, south_east)),
        (Handle::SouthEast, south_east),
        (Handle::South, midpoint(south_east, south_west)),
        (Handle::SouthWest, south_west),
        (Handle::West, midpoint(south_west, north_west)),
    ]
}

fn vello_color(color: [u8; 4]) -> VelloColor {
    VelloColor::from_rgba8(color[0], color[1], color[2], color[3])
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
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, RgbaImage};

    fn rgba_png(size: (u32, u32), color: [u8; 4]) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(size.0, size.1, image::Rgba(color)))
            .write_to(&mut output, ImageFormat::Png)
            .unwrap();
        output.into_inner()
    }

    #[test]
    fn handles_keep_a_constant_screen_size() {
        let frame = Frame::new(10.0, 20.0, 100.0, 50.0);
        let camera = Camera::new(4.0, [12.0, 8.0]).unwrap();
        let positions = handle_positions(frame, camera);
        assert_eq!(positions[0].0, Handle::NorthWest);
        assert_eq!(positions[4].0, Handle::SouthEast);
        assert_eq!(HANDLE_SIZE, 8.0);
    }

    #[test]
    fn renders_an_in_memory_scene_to_a_host_owned_device() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Some((device, queue)) = pollster::block_on(async {
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .await
                .ok()?;
            adapter
                .request_device(&wgpu::DeviceDescriptor::default())
                .await
                .ok()
        }) else {
            return;
        };

        let mut session = Session::memory().unwrap();
        let mut commands = session.commands();
        let page = commands
            .add_page("page", rgba_png((16, 12), [21, 34, 55, 255]))
            .unwrap();
        commands
            .set_asset(
                page,
                PageAsset::Clean,
                Some(rgba_png((16, 12), [89, 144, 233, 255])),
            )
            .unwrap();
        session.apply(commands).unwrap();

        let (wake, woke) = std::sync::mpsc::channel();
        let mut canvas = Canvas::new(
            CanvasGpu {
                device: Arc::new(device),
                queue: Arc::new(queue),
            },
            Arc::new(move || {
                let _ = wake.send(());
            }),
        )
        .unwrap();
        canvas.set_view(crate::ViewState {
            size: PhysicalSize::new(64, 48),
            camera: Camera::contain(PhysicalSize::new(64, 48), session.page(page).unwrap().size),
            display: crate::DisplayState::default(),
        });
        canvas.show_page(&session, page).unwrap();
        for _ in 0..2 {
            woke.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        let now = Instant::now();
        let frame = canvas.render(now).unwrap();
        assert_eq!(frame.size, PhysicalSize::new(64, 48));
        assert!(frame.generation > 0);
        let generation = frame.generation;
        assert_eq!(
            canvas.render(Instant::now()).unwrap().generation,
            generation
        );
        canvas.set_view(crate::ViewState {
            size: PhysicalSize::new(64, 48),
            camera: Camera::contain(PhysicalSize::new(64, 48), session.page(page).unwrap().size),
            display: crate::DisplayState {
                page: PageView::EditableClean,
                ..crate::DisplayState::default()
            },
        });
        assert!(canvas.render(now).unwrap().needs_redraw);
        assert!(
            !canvas
                .render(now + Duration::from_millis(181))
                .unwrap()
                .needs_redraw
        );
        let mut edit = session.edit();
        let image = edit
            .page(page)
            .unwrap()
            .add_image(
                Frame::new(2.0, 2.0, 6.0, 4.0),
                "stamp",
                rgba_png((6, 4), [233, 121, 52, 255]),
            )
            .unwrap();
        let changes = edit.commit().unwrap();
        canvas.sync(&session, &changes).unwrap();
        assert_eq!(
            canvas.hit_test(canvas.page_to_screen(PagePoint::new(4.0, 3.0))),
            Some(HitTarget::Element(image))
        );
        woke.recv_timeout(Duration::from_secs(2)).unwrap();
        canvas.render(now + Duration::from_millis(182)).unwrap();
        canvas.set_overlays(OverlayState {
            guides: vec![Guide::Vertical(4.0)],
            ..OverlayState::default()
        });
        assert!(canvas.render(Instant::now()).unwrap().generation > generation);
        canvas.clear_page();
        assert!(
            canvas
                .screen_to_page(PhysicalPoint::new(1.0, 1.0))
                .is_none()
        );
        assert!(canvas.hit_test(PhysicalPoint::new(1.0, 1.0)).is_none());
    }
}
