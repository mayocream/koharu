//! Rendering of compiled compositions into reusable vector frames.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use koharu_scene::{
    Asset, BlobId, Change, ComponentOwner, EntityChange, EntityId, LanguageTag, RelationChange,
    Revision, Snapshot,
};
use parking_lot::Mutex;
use rayon::prelude::*;
use vello::{
    Scene,
    kurbo::{Affine, Rect, Vec2},
    peniko::{Blob, Fill, ImageAlphaType, ImageData, ImageFormat, Mix},
};

use crate::{
    Composition, Error, FontFaceInfo, RenderDependency, RenderDiagnostic, RenderRequest,
    RenderTheme, Result, TextLayout, TextRenderOptions, WritingMode,
    compositor::{ImageLayer, Layer, RenderBounds},
    fonts::Fonts,
};

const DEFAULT_CACHED_FRAMES: usize = 8;
const DEFAULT_IMAGE_CACHE_BYTES: usize = 512 * 1024 * 1024;

/// Resolves composition layers into retained Vello frames.
pub struct SceneRenderer {
    fonts: Arc<Fonts>,
    images: Mutex<DecodedImageCache>,
    text_renderer: crate::TextRenderer,
    frames: Mutex<FrameCache>,
}

impl SceneRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            fonts: Fonts::shared(),
            images: Mutex::new(DecodedImageCache::new(DEFAULT_IMAGE_CACHE_BYTES)),
            text_renderer: crate::TextRenderer::new(),
            frames: Mutex::new(FrameCache::new(DEFAULT_CACHED_FRAMES)),
        }
    }

    pub async fn available_fonts() -> Result<Vec<FontFaceInfo>> {
        Fonts::shared().faces().await.map_err(Error::FontResource)
    }

    pub async fn font_preview(post_script_name: &str) -> Result<FontPreview> {
        const FONT_SIZE: f32 = 24.0;
        const PADDING: f32 = 6.0;

        let font = Fonts::shared()
            .by_post_script_name(post_script_name)
            .await
            .map_err(Error::FontResource)?;
        let label = font.family_name().to_owned();
        let layout = TextLayout::new(&font)
            .with_font_size(FONT_SIZE)
            .run(&label)
            .map_err(Error::FontResource)?;
        let width = (layout.width + PADDING * 2.0).ceil().max(1.0) as u32;
        let height = (layout.height + PADDING * 2.0).ceil().max(1.0) as u32;
        let mut scene = Scene::new();
        crate::TextRenderer::new().render(
            &mut scene,
            &layout,
            WritingMode::Horizontal,
            &TextRenderOptions::default(),
            Affine::translate((f64::from(PADDING), f64::from(PADDING))),
        );
        Ok(FontPreview {
            scene,
            width,
            height,
        })
    }

    #[must_use]
    pub const fn text_renderer(&self) -> &crate::TextRenderer {
        &self.text_renderer
    }

    pub fn render(&self, snapshot: &Snapshot, composition: &Composition) -> Result<Arc<Frame>> {
        if composition.revision() != snapshot.revision() {
            return Err(Error::invalid(format!(
                "composition revision {} does not match scene revision {}",
                composition.revision(),
                snapshot.revision()
            )));
        }
        let key = FrameKey::new(
            composition.revision(),
            self.fonts.generation(),
            &composition.request,
        );
        if let Some(frame) = self.frames.lock().get(&key) {
            return Ok(frame);
        }
        let frame = Arc::new(Frame::render(
            composition,
            snapshot,
            self,
            &composition.request.theme,
            &self.text_renderer,
        )?);
        Ok(self.frames.lock().insert(key, frame))
    }

    pub fn clear_cache(&self) {
        self.frames.lock().clear();
        self.images.lock().clear();
    }

    /// Advances unaffected cached frames to the new revision and removes stale ones.
    pub fn apply_changes(&self, changes: &Change) {
        self.frames.lock().apply_changes(changes);
    }
}

impl Default for SceneRenderer {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FontPreview {
    scene: Scene,
    width: u32,
    height: u32,
}

impl FontPreview {
    #[must_use]
    pub const fn scene(&self) -> &Scene {
        &self.scene
    }

    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VisualLayerKind {
    Image,
    Cleanup,
    Paint,
    Text,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualLayer {
    pub entity: EntityId,
    pub kind: VisualLayerKind,
    pub name: Option<String>,
    pub bounds: RenderBounds,
    pub font_size: Option<f32>,
    pub text: Option<VisualText>,
}

/// Text presentation resolved by the renderer for downstream editable exports.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualText {
    pub text: String,
    pub language: Option<LanguageTag>,
    /// The resolved text run before rotation, in page coordinates.
    pub rendered_bounds: RenderBounds,
    pub layout_bounds: RenderBounds,
    pub post_script_fonts: Vec<String>,
    pub font_size: f32,
    pub color: [u8; 4],
    pub alignment: crate::TextAlign,
    pub writing_mode: WritingMode,
    pub angle_degrees: f32,
}

pub struct Frame {
    revision: Revision,
    page: EntityId,
    width: u32,
    height: u32,
    left: i32,
    top: i32,
    scene: Arc<Scene>,
    entity_scenes: HashMap<EntityId, Vec<Arc<Scene>>>,
    layers: Vec<VisualLayer>,
    dependencies: Vec<RenderDependency>,
    diagnostics: Vec<RenderDiagnostic>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Frame")
            .field("revision", &self.revision)
            .field("page", &self.page)
            .field("size", &(self.width, self.height))
            .field("origin", &(self.left, self.top))
            .field("layers", &self.layers)
            .field("dependencies", &self.dependencies)
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

impl Frame {
    fn render(
        composition: &Composition,
        snapshot: &Snapshot,
        renderer: &SceneRenderer,
        theme: &RenderTheme,
        text_renderer: &crate::TextRenderer,
    ) -> Result<Self> {
        let rendered = composition
            .layers
            .par_iter()
            .map(|layer| render_layer(layer, composition, snapshot, renderer, theme, text_renderer))
            .collect::<Result<Vec<_>>>()?;
        let mut scene = Scene::new();
        let mut entity_scenes = HashMap::<EntityId, Vec<Arc<Scene>>>::new();
        let mut layers = Vec::with_capacity(rendered.len());
        let mut diagnostics = composition.diagnostics.clone();
        for layer in rendered {
            let entity = layer.layer.entity;
            let layer_scene = Arc::new(layer.scene);
            scene.append(&layer_scene, None);
            entity_scenes.entry(entity).or_default().push(layer_scene);
            layers.push(layer.layer);
            diagnostics.extend(layer.diagnostics);
        }
        Ok(Self {
            revision: composition.revision,
            page: composition.page,
            width: composition.width,
            height: composition.height,
            left: 0,
            top: 0,
            scene: Arc::new(scene),
            entity_scenes,
            layers,
            dependencies: composition.dependencies.clone(),
            diagnostics,
        })
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn page(&self) -> EntityId {
        self.page
    }

    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    #[must_use]
    pub const fn origin(&self) -> (i32, i32) {
        (self.left, self.top)
    }

    #[must_use]
    pub fn layers(&self) -> &[VisualLayer] {
        &self.layers
    }

    #[must_use]
    pub fn dependencies(&self) -> &[RenderDependency] {
        &self.dependencies
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[RenderDiagnostic] {
        &self.diagnostics
    }

    /// Appends the rendered frame to a caller-owned Vello scene without rasterization.
    pub fn append_to(&self, scene: &mut Scene, transform: Option<Affine>) {
        scene.append(&self.scene, transform);
    }

    /// Appends every visual layer owned by one entity.
    ///
    /// Interactive consumers use this to apply a transient transform without
    /// repeating image decoding, text shaping, or scene rendering.
    pub fn append_entity_to(
        &self,
        entity: EntityId,
        scene: &mut Scene,
        transform: Option<Affine>,
    ) -> bool {
        let Some(layers) = self.entity_scenes.get(&entity) else {
            return false;
        };
        for layer in layers {
            scene.append(layer, transform);
        }
        true
    }

    pub(crate) fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Returns a tightly cropped vector frame for one entity.
    ///
    /// The returned origin remains in page coordinates while its Vello scene
    /// is translated into frame-local coordinates.
    pub fn entity(&self, entity: EntityId) -> Result<Option<Self>> {
        let mut bounds = self
            .layers
            .iter()
            .filter(|layer| layer.entity == entity)
            .map(|layer| layer.bounds);
        let Some(first) = bounds.next() else {
            return Ok(None);
        };
        let (mut left, mut top) = (first.x, first.y);
        let (mut right, mut bottom) = (first.x + first.width, first.y + first.height);
        for bounds in bounds {
            left = left.min(bounds.x);
            top = top.min(bounds.y);
            right = right.max(bounds.x + bounds.width);
            bottom = bottom.max(bounds.y + bounds.height);
        }
        if ![left, top, right, bottom].into_iter().all(f32::is_finite) {
            return Err(Error::invalid(format!(
                "visual layer bounds are not finite for entity {entity}"
            )));
        }
        let left = left.floor() as i32;
        let top = top.floor() as i32;
        let right = right.ceil() as i32;
        let bottom = bottom.ceil() as i32;
        let width = u32::try_from((i64::from(right) - i64::from(left)).max(1))
            .map_err(|_| Error::invalid("visual layer width exceeds u32"))?;
        let height = u32::try_from((i64::from(bottom) - i64::from(top)).max(1))
            .map_err(|_| Error::invalid("visual layer height exceeds u32"))?;
        let mut scene = Scene::new();
        self.append_entity_to(
            entity,
            &mut scene,
            Some(Affine::translate((
                f64::from(self.left) - f64::from(left),
                f64::from(self.top) - f64::from(top),
            ))),
        );
        let layer_scene = Arc::new(scene);
        Ok(Some(Self {
            revision: self.revision,
            page: self.page,
            width,
            height,
            left,
            top,
            scene: layer_scene.clone(),
            entity_scenes: HashMap::from([(entity, vec![layer_scene])]),
            layers: self
                .layers
                .iter()
                .filter(|layer| layer.entity == entity)
                .cloned()
                .collect(),
            dependencies: self.dependencies.clone(),
            diagnostics: self.diagnostics.clone(),
        }))
    }

    pub(crate) fn at_revision(&self, revision: Revision) -> Self {
        Self {
            revision,
            page: self.page,
            width: self.width,
            height: self.height,
            left: self.left,
            top: self.top,
            scene: self.scene.clone(),
            entity_scenes: self.entity_scenes.clone(),
            layers: self.layers.clone(),
            dependencies: self.dependencies.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn empty_for_test(
        revision: Revision,
        page: EntityId,
        dependencies: Vec<RenderDependency>,
    ) -> Self {
        Self {
            revision,
            page,
            width: 100,
            height: 100,
            left: 0,
            top: 0,
            scene: Arc::new(Scene::new()),
            entity_scenes: HashMap::new(),
            layers: Vec::new(),
            dependencies,
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct FrameKey {
    revision: Revision,
    font_generation: u64,
    request: RenderRequest,
}

impl FrameKey {
    fn new(revision: Revision, font_generation: u64, request: &RenderRequest) -> Self {
        Self {
            revision,
            font_generation,
            request: request.clone(),
        }
    }
}

struct FrameEntry {
    key: FrameKey,
    frame: Arc<Frame>,
}

struct FrameCache {
    entries: VecDeque<FrameEntry>,
    capacity: usize,
}

impl FrameCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn get(&mut self, key: &FrameKey) -> Option<Arc<Frame>> {
        let position = self.entries.iter().position(|entry| entry.key == *key)?;
        let entry = self.entries.remove(position)?;
        let frame = entry.frame.clone();
        self.entries.push_back(entry);
        Some(frame)
    }

    fn insert(&mut self, key: FrameKey, frame: Arc<Frame>) -> Arc<Frame> {
        if let Some(existing) = self.get(&key) {
            return existing;
        }
        if self.capacity == 0 {
            return frame;
        }
        while self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(FrameEntry {
            key,
            frame: frame.clone(),
        });
        frame
    }

    fn apply_changes(&mut self, changes: &Change) {
        let invalidate_all = !changes.relations.is_empty()
            || changes
                .components
                .iter()
                .any(|change| change.owner == ComponentOwner::Project)
            || changes
                .entities
                .iter()
                .any(|change| matches!(change, EntityChange::Inserted(_)));
        if invalidate_all {
            self.entries
                .retain(|entry| entry.key.revision != changes.from);
            return;
        }
        self.entries.retain_mut(|entry| {
            if entry.key.revision != changes.from {
                return true;
            }
            let depends_on_entity = |entity| {
                entry
                    .frame
                    .dependencies()
                    .contains(&RenderDependency::Entity(entity))
            };
            let entity_changed = changes.entities.iter().any(|change| match *change {
                EntityChange::Inserted(entity) | EntityChange::Removed(entity) => {
                    depends_on_entity(entity)
                }
            }) || changes.hierarchy.iter().copied().any(depends_on_entity)
                || changes.components.iter().any(|change| match change.owner {
                    ComponentOwner::Project => false,
                    ComponentOwner::Entity(entity) => depends_on_entity(entity),
                    ComponentOwner::Relation(relation) => entry
                        .frame
                        .dependencies()
                        .contains(&RenderDependency::Relation(relation)),
                });
            let relation_changed = changes.relations.iter().any(|change| {
                let relation = match *change {
                    RelationChange::Inserted(id)
                    | RelationChange::Removed(id)
                    | RelationChange::Changed(id) => id,
                };
                entry
                    .frame
                    .dependencies()
                    .contains(&RenderDependency::Relation(relation))
            });
            if entity_changed || relation_changed {
                false
            } else {
                entry.key.revision = changes.to;
                entry.frame = Arc::new(entry.frame.at_revision(changes.to));
                true
            }
        });
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

pub(crate) struct RenderedLayer {
    pub(crate) scene: Scene,
    pub(crate) layer: VisualLayer,
    pub(crate) diagnostics: Vec<RenderDiagnostic>,
}

fn render_layer(
    layer: &Layer,
    composition: &Composition,
    snapshot: &Snapshot,
    renderer: &SceneRenderer,
    theme: &RenderTheme,
    text_renderer: &crate::TextRenderer,
) -> Result<RenderedLayer> {
    match layer {
        Layer::Image(layer) => render_image(layer, composition, snapshot, renderer),
        Layer::Text(layer) => text_renderer.render_layer(layer, &renderer.fonts, theme),
    }
}

fn render_image(
    layer: &ImageLayer,
    composition: &Composition,
    snapshot: &Snapshot,
    renderer: &SceneRenderer,
) -> Result<RenderedLayer> {
    let image = renderer.image(snapshot, &layer.asset)?;
    if layer.is_base && (image.width != composition.width || image.height != composition.height) {
        return Err(Error::invalid(format!(
            "base image for page {} is {}x{}, expected {}x{}",
            composition.page, image.width, image.height, composition.width, composition.height
        )));
    }
    let pixels: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(ImageBytes(image.pixels.clone()));
    let data = ImageData {
        data: Blob::new(pixels),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: image.width,
        height: image.height,
    };
    let transform = Affine::scale_non_uniform(
        f64::from(layer.bounds.width) / f64::from(image.width),
        f64::from(layer.bounds.height) / f64::from(image.height),
    )
    .then_translate(Vec2::new(
        f64::from(layer.bounds.x),
        f64::from(layer.bounds.y),
    ));
    let mut scene = Scene::new();
    if layer.opacity < 1.0 {
        scene.push_layer(
            Fill::NonZero,
            Mix::Normal,
            layer.opacity,
            transform,
            &Rect::new(0.0, 0.0, f64::from(image.width), f64::from(image.height)),
        );
    }
    scene.draw_image(&data, transform);
    if layer.opacity < 1.0 {
        scene.pop_layer();
    }
    Ok(RenderedLayer {
        scene,
        layer: VisualLayer {
            entity: layer.entity,
            kind: layer.kind,
            name: layer.name.clone(),
            bounds: layer.bounds.into(),
            font_size: None,
            text: None,
        },
        diagnostics: Vec::new(),
    })
}

struct ImageBytes(Arc<[u8]>);

impl AsRef<[u8]> for ImageBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl SceneRenderer {
    fn image(&self, snapshot: &Snapshot, asset: &Asset) -> Result<Arc<DecodedImage>> {
        if let Some(image) = self.images.lock().get(asset.blob) {
            return Ok(image);
        }
        let bytes = snapshot.read_blob(asset.blob)?;
        let decoded = image::load_from_memory(&bytes)
            .map_err(|source| Error::Image {
                blob: asset.blob,
                source,
            })?
            .into_rgba8();
        if let (Some(expected_width), Some(expected_height)) =
            (asset.metadata.width, asset.metadata.height)
            && (decoded.width() != expected_width || decoded.height() != expected_height)
        {
            return Err(Error::invalid(format!(
                "blob {} decoded as {}x{}, expected {}x{}",
                asset.blob,
                decoded.width(),
                decoded.height(),
                expected_width,
                expected_height
            )));
        }
        let image = Arc::new(DecodedImage {
            width: decoded.width(),
            height: decoded.height(),
            pixels: Arc::from(decoded.into_raw()),
        });
        self.images.lock().insert(asset.blob, image.clone());
        Ok(image)
    }
}

struct DecodedImage {
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

impl DecodedImage {
    fn byte_len(&self) -> usize {
        self.pixels.len()
    }
}

struct CachedImage {
    image: Arc<DecodedImage>,
    last_used: u64,
}

struct DecodedImageCache {
    entries: HashMap<BlobId, CachedImage>,
    max_bytes: usize,
    bytes: usize,
    clock: u64,
}

impl DecodedImageCache {
    fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_bytes,
            bytes: 0,
            clock: 0,
        }
    }

    fn get(&mut self, id: BlobId) -> Option<Arc<DecodedImage>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(&id)?;
        entry.last_used = self.clock;
        Some(entry.image.clone())
    }

    fn insert(&mut self, id: BlobId, image: Arc<DecodedImage>) {
        let image_bytes = image.byte_len();
        if self.max_bytes == 0 || image_bytes > self.max_bytes {
            return;
        }
        if let Some(previous) = self.entries.remove(&id) {
            self.bytes = self.bytes.saturating_sub(previous.image.byte_len());
        }
        while self.bytes.saturating_add(image_bytes) > self.max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(id, _)| *id)
            else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(removed.image.byte_len());
            }
        }
        self.clock = self.clock.wrapping_add(1);
        self.bytes += image_bytes;
        self.entries.insert(
            id,
            CachedImage {
                image,
                last_used: self.clock,
            },
        );
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use koharu_scene::{
        At, Authored, BubbleRegion, FitsTo, Geometry, LanguageTag, Origin, PageDraft, Session,
        SourceText, TextAlignment, TextLayout, TextLayoutKind, Typography,
    };

    use super::*;
    use crate::{Compositor, RenderRequest};

    fn render_frame(
        composition: &Composition,
        snapshot: &Snapshot,
        renderer: &SceneRenderer,
        theme: &RenderTheme,
    ) -> Result<Frame> {
        Frame::render(
            composition,
            snapshot,
            renderer,
            theme,
            &crate::TextRenderer::new(),
        )
    }

    fn cached_frame(
        revision: Revision,
        page: EntityId,
        dependencies: Vec<RenderDependency>,
    ) -> Arc<Frame> {
        Arc::new(Frame::empty_for_test(revision, page, dependencies))
    }

    #[test]
    fn change_sets_reuse_only_unaffected_frames() {
        let mut session = Session::memory().unwrap();
        let mut ids = None;
        let create = session
            .snapshot()
            .patch(|edit| {
                let first_page = edit.add_page(PageDraft::new("first", 100.0, 100.0), At::End)?;
                let first_entity = edit.add_entity(first_page, At::End)?;
                edit.set(first_entity, &Geometry::rectangle(0.0, 0.0, 10.0, 10.0))?;
                let second_page = edit.add_page(PageDraft::new("second", 100.0, 100.0), At::End)?;
                let second_entity = edit.add_entity(second_page, At::End)?;
                edit.set(second_entity, &Geometry::rectangle(0.0, 0.0, 10.0, 10.0))?;
                ids = Some((first_page, first_entity, second_entity));
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(create).unwrap().snapshot;
        let (first_page, first_entity, second_entity) = ids.unwrap();
        let request = RenderRequest::transparent(first_page);
        let mut cache = FrameCache::new(2);
        cache.insert(
            FrameKey::new(snapshot.revision(), 0, &request),
            cached_frame(
                snapshot.revision(),
                first_page,
                vec![
                    RenderDependency::Entity(first_page),
                    RenderDependency::Entity(first_entity),
                ],
            ),
        );

        let unrelated = snapshot
            .patch(|edit| edit.set(second_entity, &Geometry::rectangle(1.0, 1.0, 10.0, 10.0)))
            .unwrap();
        let commit = session.commit(unrelated).unwrap();
        cache.apply_changes(&commit.changes);
        let reused = cache
            .get(&FrameKey::new(commit.snapshot.revision(), 0, &request))
            .expect("unaffected frame should advance to the new revision");
        assert_eq!(reused.revision(), commit.snapshot.revision());

        let relevant = commit
            .snapshot
            .patch(|edit| edit.set(first_entity, &Geometry::rectangle(2.0, 2.0, 10.0, 10.0)))
            .unwrap();
        let commit = session.commit(relevant).unwrap();
        cache.apply_changes(&commit.changes);

        assert!(
            cache
                .get(&FrameKey::new(commit.snapshot.revision(), 0, &request))
                .is_none()
        );
    }

    fn text_fixture(
        balloon_width: f64,
        balloon_height: f64,
        text: &str,
        font_size: f32,
    ) -> (koharu_scene::Snapshot, Composition, EntityId) {
        let mut session = Session::memory().unwrap();
        let mut text_entity = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("page", 300.0, 200.0), At::End)?;
                let bubble = edit.add_analysis_region::<BubbleRegion>(
                    page,
                    At::End,
                    &Geometry::rectangle(10.0, 10.0, balloon_width, balloon_height),
                    None,
                )?;
                let content = edit.add_text_content(page, At::End)?;
                edit.set(
                    content,
                    &SourceText {
                        text: Authored::user(text.to_owned()),
                        language: Some(LanguageTag::new("en")?),
                    },
                )?;
                let entity = edit.add_text_layer(
                    page,
                    At::End,
                    content,
                    &TextLayout {
                        origin: Origin::User,
                        kind: TextLayoutKind::Paragraph,
                    },
                )?;
                edit.set(
                    entity,
                    &Geometry::rectangle(10.0, 10.0, balloon_width, balloon_height),
                )?;
                edit.set(
                    entity,
                    &Typography {
                        origin: Origin::User,
                        preferred_font: None,
                        font_weight: None,
                        size: Some(font_size),
                        auto_fit: false,
                        color: None,
                        stroke_color: None,
                        stroke_width: None,
                        alignment: Some(TextAlignment::Center),
                        writing_mode: Some(koharu_scene::WritingMode::Horizontal),
                        extensions: Default::default(),
                    },
                )?;
                edit.relate::<FitsTo>(entity, bubble)?;
                text_entity = Some(entity);
                Ok(())
            })
            .unwrap();
        let snapshot = session.commit(patch).unwrap().snapshot;
        let entity = text_entity.unwrap();
        let page = snapshot.parent(entity).unwrap().unwrap();
        let composition = Compositor::new()
            .compile(&snapshot, &RenderRequest::transparent(page))
            .unwrap();
        (snapshot, composition, entity)
    }

    #[test]
    fn explicit_font_size_skips_auto_fit_and_reports_overflow() {
        let (snapshot, composition, entity) =
            text_fixture(40.0, 18.0, "This dialogue cannot fit", 18.0);
        let theme = RenderTheme {
            text_inset: [0.0; 4],
            ..RenderTheme::default()
        };

        let frame = render_frame(&composition, &snapshot, &SceneRenderer::new(), &theme).unwrap();
        let rendered = frame
            .layers()
            .iter()
            .find(|layer| layer.entity == entity)
            .unwrap();

        assert_eq!(rendered.font_size, Some(18.0));
        assert!(frame.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RenderDiagnostic::TextOverflow { entity: found, .. } if *found == entity
        )));
        assert_eq!(frame.entity_scenes[&entity][0].encoding().n_clips, 0);
    }

    #[test]
    fn free_text_auto_fits_the_exact_original_block_without_balloon_air() {
        let (snapshot, mut composition, entity) =
            text_fixture(40.0, 18.0, "This free text must shrink", 18.0);
        let Layer::Text(layer) = &mut composition.layers[0] else {
            panic!("expected a text layer");
        };
        layer.balloon_contour = None;
        layer.font_size = None;
        let original_bounds = layer.bounds;
        let theme = RenderTheme {
            minimum_font_size: 1.0,
            text_inset: [100.0; 4],
            ..RenderTheme::default()
        };

        let frame = render_frame(&composition, &snapshot, &SceneRenderer::new(), &theme).unwrap();
        let rendered = frame
            .layers()
            .iter()
            .find(|rendered| rendered.entity == entity)
            .unwrap();

        assert!(rendered.font_size.unwrap() < theme.font_size);
        assert!(rendered.bounds.x >= original_bounds.x - f32::EPSILON);
        assert!(rendered.bounds.y >= original_bounds.y - f32::EPSILON);
        assert!(rendered.bounds.width <= original_bounds.width + f32::EPSILON);
        assert!(rendered.bounds.height <= original_bounds.height + f32::EPSILON);
        assert!(!frame.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RenderDiagnostic::TextOverflow { entity: found, .. } if *found == entity
        )));
    }

    #[test]
    fn automatic_balloon_text_can_grow_beyond_the_theme_font_size() {
        let (snapshot, mut composition, entity) = text_fixture(240.0, 120.0, "Hi", 18.0);
        let Layer::Text(layer) = &mut composition.layers[0] else {
            panic!("expected a text layer");
        };
        layer.font_size = None;
        let theme = RenderTheme {
            text_inset: [0.0; 4],
            ..RenderTheme::default()
        };

        let frame = render_frame(&composition, &snapshot, &SceneRenderer::new(), &theme).unwrap();
        let rendered = frame
            .layers()
            .iter()
            .find(|rendered| rendered.entity == entity)
            .unwrap();

        assert!(rendered.font_size.unwrap() > theme.font_size);
    }

    #[test]
    fn entity_frame_is_cropped_once_in_page_coordinates() {
        let (snapshot, composition, entity) = text_fixture(240.0, 120.0, "Cropped", 18.0);
        let frame = render_frame(
            &composition,
            &snapshot,
            &SceneRenderer::new(),
            &RenderTheme::default(),
        )
        .unwrap();
        let bounds = frame
            .layers()
            .iter()
            .find(|layer| layer.entity == entity)
            .unwrap()
            .bounds;
        let expected_origin = (bounds.x.floor() as i32, bounds.y.floor() as i32);
        let expected_size = (
            (bounds.x + bounds.width).ceil() as u32 - expected_origin.0 as u32,
            (bounds.y + bounds.height).ceil() as u32 - expected_origin.1 as u32,
        );

        let entity_frame = frame.entity(entity).unwrap().unwrap();
        assert_eq!(entity_frame.origin(), expected_origin);
        assert_eq!(entity_frame.size(), expected_size);

        let nested = entity_frame.entity(entity).unwrap().unwrap();
        assert_eq!(nested.origin(), entity_frame.origin());
        assert_eq!(nested.size(), entity_frame.size());
    }

    #[test]
    fn rendering_reports_text_below_the_readability_floor() {
        let (snapshot, composition, entity) = text_fixture(240.0, 120.0, "Small dialogue", 8.0);
        let theme = RenderTheme {
            minimum_font_size: 9.0,
            text_inset: [0.0; 4],
            ..RenderTheme::default()
        };

        let frame = render_frame(&composition, &snapshot, &SceneRenderer::new(), &theme).unwrap();

        assert!(frame.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            RenderDiagnostic::TextBelowReadableSize {
                entity: found,
                font_size,
                minimum_font_size,
            } if *found == entity && *font_size == 8.0 && *minimum_font_size == 9.0
        )));
    }

    #[test]
    fn rendering_rotates_text_and_reported_bounds() {
        let (snapshot, composition, entity) = text_fixture(240.0, 120.0, "Rotated text", 18.0);
        let theme = RenderTheme {
            text_inset: [0.0; 4],
            ..RenderTheme::default()
        };
        let renderer = SceneRenderer::new();
        let baseline = render_frame(&composition, &snapshot, &renderer, &theme).unwrap();
        let baseline_bounds = baseline
            .layers()
            .iter()
            .find(|rendered| rendered.entity == entity)
            .unwrap()
            .bounds;

        let mut rotated_composition = composition.clone();
        let Layer::Text(layer) = &mut rotated_composition.layers[0] else {
            panic!("expected a text layer");
        };
        layer.angle_degrees = 90.0;
        let rotated = render_frame(&rotated_composition, &snapshot, &renderer, &theme).unwrap();
        let rotated_bounds = rotated
            .layers()
            .iter()
            .find(|rendered| rendered.entity == entity)
            .unwrap()
            .bounds;

        assert!((rotated_bounds.width - baseline_bounds.height).abs() < 1e-4);
        assert!((rotated_bounds.height - baseline_bounds.width).abs() < 1e-4);
        assert!(
            (rotated_bounds.x + rotated_bounds.width * 0.5
                - baseline_bounds.x
                - baseline_bounds.width * 0.5)
                .abs()
                < 1e-4
        );
        assert!(
            (rotated_bounds.y + rotated_bounds.height * 0.5
                - baseline_bounds.y
                - baseline_bounds.height * 0.5)
                .abs()
                < 1e-4
        );
    }
}
