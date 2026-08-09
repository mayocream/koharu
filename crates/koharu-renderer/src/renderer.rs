//! Retained page composition and asynchronous resource orchestration.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
};

use anyhow::Context;
use koharu_scene::{
    BlobId, BubbleRegion, Change, EntityId, Geometry, Group, LanguageTag, PixelFormat, PixelLayer,
    RegionSpec, Revision, Snapshot, TextLayout as AuthoredTextLayout, Visibility,
};
use parking_lot::Mutex;
use rayon::prelude::*;
use skrifa::{
    GlyphId, MetadataProvider,
    instance::Size,
    outline::{DrawSettings, OutlinePen},
};
use tokio::sync::OnceCell as AsyncOnceCell;
use vello::{
    Scene,
    kurbo::{Affine, BezPath, Rect},
    peniko::{Blob as PenikoBlob, Fill, ImageAlphaType, ImageData, ImageFormat, Mix},
};

use crate::{
    Error, FontFamily, FontStyle, Result, TextLayout,
    bubble::{self, GeometryFrame, LayoutBox},
    fonts::{FontPreview, Fonts},
    rasterizer::{RasterImage, RasterOptions, Rasterizer, rgba},
    text_renderer::{RenderedTextNode, TextNodeDescriptor, TextRenderer},
};

const MAX_SURFACE_DIMENSION: u32 = 32_768;
const MAX_SURFACE_PIXELS: u64 = 268_435_456;
const DEFAULT_IMAGE_CACHE_BYTES: usize = 512 * 1024 * 1024;
const DEFAULT_RETAINED_NODES: usize = 2_048;

/// The sole rendering owner. Clones share caches, font state, workers, and GPU state.
#[derive(Clone)]
pub struct Renderer {
    inner: Arc<RendererInner>,
}

struct RendererInner {
    fonts: Arc<Fonts>,
    images: Arc<Mutex<DecodedImageCache>>,
    nodes: Arc<Mutex<RetainedNodeCache>>,
    decode_pool: OnceLock<rayon::ThreadPool>,
    rasterizer: AsyncOnceCell<Arc<Rasterizer>>,
}

impl Renderer {
    /// Construction performs no font discovery, storage I/O, decoding, or GPU setup.
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: Arc::new(RendererInner {
                fonts: Arc::new(Fonts::new()),
                images: Arc::new(Mutex::new(DecodedImageCache::new(
                    DEFAULT_IMAGE_CACHE_BYTES,
                ))),
                nodes: Arc::new(Mutex::new(RetainedNodeCache::new(DEFAULT_RETAINED_NODES))),
                decode_pool: OnceLock::new(),
                rasterizer: AsyncOnceCell::new(),
            }),
        })
    }

    /// Composes one page, resolving storage and font resources away from the caller's executor.
    pub async fn compose(&self, snapshot: &Snapshot, page: EntityId) -> Result<Composition> {
        self.compose_inner(snapshot, page, None).await
    }

    /// Re-composes a changed revision while retaining equal entity descriptors.
    pub async fn update(
        &self,
        previous: &Composition,
        snapshot: &Snapshot,
        change: &Change,
    ) -> Result<Composition> {
        if change.from != previous.revision() || change.to != snapshot.revision() {
            return Err(Error::invalid(format!(
                "renderer update revisions do not line up: composition {}, change {} -> {}, snapshot {}",
                previous.revision(),
                change.from,
                change.to,
                snapshot.revision()
            )));
        }
        if change.from == change.to {
            return Ok(previous.clone());
        }
        self.compose_inner(snapshot, previous.page(), Some(previous.clone()))
            .await
    }

    /// Rasterizes the exact vector content stored by a composition.
    pub async fn rasterize(
        &self,
        composition: &Composition,
        options: &RasterOptions,
    ) -> Result<RasterImage> {
        let rasterizer = self
            .inner
            .rasterizer
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(Rasterizer::new)
                    .await
                    .map_err(|error| Error::Backend(error.into()))?
                    .map(Arc::new)
            })
            .await?
            .clone();
        let composition = composition.clone();
        let options = *options;
        tokio::task::spawn_blocking(move || rasterizer.rasterize(&composition, options))
            .await
            .map_err(|error| Error::Backend(error.into()))?
    }

    /// Lists system and bundled families through the renderer-owned font library.
    pub async fn available_fonts(&self) -> Result<Vec<FontFamily>> {
        self.inner
            .fonts
            .families()
            .await
            .map_err(Error::FontResource)
    }

    /// Produces an encoded preview for a font family without exposing font ownership.
    pub async fn font_preview(&self, family_name: &str) -> Result<Vec<u8>> {
        const FONT_SIZE: f32 = 24.0;
        const PREVIEW_HEIGHT: u32 = 96;
        let font = match self
            .inner
            .fonts
            .preview(family_name)
            .await
            .map_err(Error::FontResource)?
        {
            FontPreview::Webp(bytes) => return Ok(bytes),
            FontPreview::System(font) => *font,
        };
        let fonts = self.inner.fonts.clone();
        let label = family_name.to_owned();
        let (scene, width) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let preview_fonts = if font.renders(&label, FONT_SIZE) {
                vec![font]
            } else {
                fonts.resolve(Some("Arial"), Some(400), None, &[], &label, None)?
            };
            let measured = TextLayout::new(&preview_fonts[0])
                .with_fallback_fonts(&preview_fonts[1..])
                .with_font_size(FONT_SIZE)
                .run(&label)?;
            let font_size = FONT_SIZE * PREVIEW_HEIGHT as f32 / measured.height.max(1.0);
            let layout = TextLayout::new(&preview_fonts[0])
                .with_fallback_fonts(&preview_fonts[1..])
                .with_font_size(font_size)
                .run(&label)?;
            let width = layout.width.ceil().max(1.0) as u32;
            let mut scene = Scene::new();
            draw_font_preview(&mut scene, &layout)?;
            Ok((scene, width))
        })
        .await
        .context("font preview layout worker stopped")
        .and_then(|result| result)
        .map_err(Error::FontResource)?;
        let rasterizer = self
            .inner
            .rasterizer
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(Rasterizer::new)
                    .await
                    .map_err(|error| Error::Backend(error.into()))?
                    .map(Arc::new)
            })
            .await?
            .clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
            let image = rasterizer.rasterize_scene(
                &scene,
                width,
                PREVIEW_HEIGHT,
                [0, 0, 0, 0],
                RasterOptions::default(),
            )?;
            Ok(
                webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height())
                    .encode(85.0)
                    .to_vec(),
            )
        })
        .await
        .context("font preview raster worker stopped")
        .and_then(|result| result)
        .map_err(Error::FontResource)
    }

    async fn compose_inner(
        &self,
        snapshot: &Snapshot,
        page_id: EntityId,
        previous: Option<Composition>,
    ) -> Result<Composition> {
        let compiled = compile_page(snapshot, page_id)?;
        let previous_nodes = previous
            .as_ref()
            .filter(|composition| composition.page() == page_id)
            .map(Composition::node_map)
            .unwrap_or_default();

        let mut font_requests = Vec::new();
        let mut blobs = Vec::new();
        for layer in &compiled.layers {
            let descriptor = layer.node_descriptor();
            let reused = previous_nodes
                .get(&layer.entity)
                .is_some_and(|node| node.descriptor == *descriptor)
                || self
                    .inner
                    .nodes
                    .lock()
                    .contains(page_id, layer.entity, descriptor);
            if reused {
                continue;
            }
            match descriptor {
                NodeDescriptor::Pixel(pixel) => blobs.push(pixel.blob),
                NodeDescriptor::Text(text) => {
                    let style = match text.typography.font_style {
                        koharu_scene::FontStyle::Normal => FontStyle::Normal,
                        koharu_scene::FontStyle::Italic => FontStyle::Italic,
                        koharu_scene::FontStyle::Oblique => FontStyle::Oblique,
                    };
                    font_requests.extend(
                        text.typography
                            .font_families
                            .iter()
                            .cloned()
                            .map(|family| (family, text.typography.font_weight, style)),
                    );
                }
            }
        }
        font_requests
            .sort_by_key(|(family, weight, style)| (family.to_lowercase(), *weight, *style));
        font_requests.dedup_by(|left, right| {
            left.0.eq_ignore_ascii_case(&right.0) && left.1 == right.1 && left.2 == right.2
        });
        blobs.sort_unstable();
        blobs.dedup();

        if !font_requests.is_empty() {
            self.inner
                .fonts
                .prepare(&font_requests)
                .await
                .map_err(Error::FontResource)?;
        }
        self.load_images(snapshot, &compiled, &blobs).await?;

        let fonts = self.inner.fonts.clone();
        let images = self.inner.images.clone();
        let cache = self.inner.nodes.clone();
        tokio::task::spawn_blocking(move || {
            build_composition(compiled, previous_nodes, fonts, images, cache)
        })
        .await
        .map_err(|error| Error::Backend(error.into()))?
    }

    async fn load_images(
        &self,
        snapshot: &Snapshot,
        compiled: &CompiledPage,
        requested: &[BlobId],
    ) -> Result<()> {
        let missing = {
            let images = self.inner.images.lock();
            requested
                .iter()
                .copied()
                .filter(|blob| !images.contains(*blob))
                .collect::<Vec<_>>()
        };
        if missing.is_empty() {
            return Ok(());
        }
        let expected = compiled.layers.iter();
        let missing_set = missing.iter().copied().collect::<HashSet<_>>();
        let expected = expected
            .filter_map(|layer| match layer.node_descriptor() {
                NodeDescriptor::Pixel(pixel) if missing_set.contains(&pixel.blob) => {
                    Some((pixel.blob, pixel.expected_size))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let snapshot = snapshot.clone();
        let images = self.inner.images.clone();
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let batch = snapshot.read_blobs(missing.iter().copied())?;
            let pool = if let Some(pool) = inner.decode_pool.get() {
                pool
            } else {
                let workers = std::thread::available_parallelism()
                    .map_or(2, usize::from)
                    .saturating_sub(1)
                    .clamp(2, 8);
                let built = rayon::ThreadPoolBuilder::new()
                    .num_threads(workers)
                    .thread_name(|index| format!("koharu-image-{index}"))
                    .build()
                    .map_err(|error| Error::Backend(error.into()))?;
                let _ = inner.decode_pool.set(built);
                inner
                    .decode_pool
                    .get()
                    .expect("decode pool was initialized")
            };
            let decoded = pool.install(|| {
                batch
                    .iter()
                    .collect::<Vec<_>>()
                    .into_par_iter()
                    .map(|(blob, bytes)| {
                        decode_image(blob, bytes, expected.get(&blob).copied().flatten())
                    })
                    .collect::<Result<Vec<_>>>()
            })?;
            let mut cache = images.lock();
            for (blob, image) in decoded {
                cache.insert(blob, image);
            }
            Ok(())
        })
        .await
        .map_err(|error| Error::Backend(error.into()))?
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new().expect("lazy renderer construction cannot fail")
    }
}

/// Immutable retained output shared by canvas, layered export, and raster export.
#[derive(Clone)]
pub struct Composition(Arc<CompositionData>);

struct CompositionData {
    revision: Revision,
    page: EntityId,
    width: u32,
    height: u32,
    origin: (i32, i32),
    normalization: Affine,
    layers: Arc<[Layer]>,
    layer_index: HashMap<EntityId, usize>,
    diagnostics: Arc<[RenderDiagnostic]>,
    stats: CompositionStats,
    scene: OnceLock<Arc<Scene>>,
}

impl Composition {
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.0.revision
    }

    #[must_use]
    pub fn page(&self) -> EntityId {
        self.0.page
    }

    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }

    #[must_use]
    pub fn origin(&self) -> (i32, i32) {
        self.0.origin
    }

    #[must_use]
    pub fn layers(&self) -> &[Layer] {
        &self.0.layers
    }

    #[must_use]
    pub fn layer(&self, entity: EntityId) -> Option<&Layer> {
        self.0
            .layer_index
            .get(&entity)
            .map(|index| &self.0.layers[*index])
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[RenderDiagnostic] {
        &self.0.diagnostics
    }

    #[must_use]
    pub fn stats(&self) -> CompositionStats {
        self.0.stats
    }

    pub fn append_to(&self, scene: &mut Scene, transform: Option<Affine>) {
        let transform =
            transform.map_or(self.0.normalization, |outer| outer * self.0.normalization);
        for layer in self.layers() {
            layer.append_to(scene, Some(transform));
        }
    }

    /// Returns a one-layer composition normalized to that layer's pixel bounds.
    pub fn cropped(&self, entity: EntityId) -> Result<Option<Self>> {
        let Some(layer) = self.layer(entity).cloned() else {
            return Ok(None);
        };
        let bounds = layer.bounds();
        let left = bounds.x.floor() as i32;
        let top = bounds.y.floor() as i32;
        let right = (bounds.x + bounds.width).ceil() as i32;
        let bottom = (bounds.y + bounds.height).ceil() as i32;
        let width = (right - left).max(1) as u32;
        let height = (bottom - top).max(1) as u32;
        let mut isolated = layer.0.clone_for_isolation();
        isolated.presentation = Presentation {
            visible: true,
            opacity: 1.0,
        };
        let layers: Arc<[Layer]> = vec![Layer(Arc::new(isolated))].into();
        let layer_index = HashMap::from([(entity, 0)]);
        Ok(Some(Self(Arc::new(CompositionData {
            revision: self.revision(),
            page: self.page(),
            width,
            height,
            origin: (left, top),
            normalization: Affine::translate((-f64::from(left), -f64::from(top))),
            layers,
            layer_index,
            diagnostics: self.0.diagnostics.clone(),
            stats: self.0.stats,
            scene: OnceLock::new(),
        }))))
    }

    pub(crate) fn scene(&self) -> &Arc<Scene> {
        self.0.scene.get_or_init(|| {
            let mut scene = Scene::new();
            self.append_to(&mut scene, None);
            Arc::new(scene)
        })
    }

    fn node_map(&self) -> HashMap<EntityId, Arc<RetainedNode>> {
        self.layers()
            .iter()
            .map(|layer| (layer.entity(), layer.0.node.clone()))
            .collect()
    }
}

#[derive(Clone)]
pub struct Layer(Arc<LayerData>);

struct LayerData {
    entity: EntityId,
    geometry: Geometry,
    bounds: RenderBounds,
    presentation: Presentation,
    ancestry: Arc<[EntityId]>,
    kind: LayerKind,
    placement: Affine,
    node: Arc<RetainedNode>,
}

impl LayerData {
    fn clone_for_isolation(&self) -> Self {
        Self {
            entity: self.entity,
            geometry: self.geometry.clone(),
            bounds: self.bounds,
            presentation: self.presentation,
            ancestry: self.ancestry.clone(),
            kind: self.kind.clone(),
            placement: self.placement,
            node: self.node.clone(),
        }
    }
}

impl Layer {
    #[must_use]
    pub fn entity(&self) -> EntityId {
        self.0.entity
    }

    #[must_use]
    pub fn geometry(&self) -> &Geometry {
        &self.0.geometry
    }

    #[must_use]
    pub fn bounds(&self) -> RenderBounds {
        self.0.bounds
    }

    #[must_use]
    pub fn presentation(&self) -> Presentation {
        self.0.presentation
    }

    #[must_use]
    pub fn ancestry(&self) -> &[EntityId] {
        &self.0.ancestry
    }

    #[must_use]
    pub fn kind(&self) -> &LayerKind {
        &self.0.kind
    }

    pub fn append_to(&self, scene: &mut Scene, transform: Option<Affine>) {
        self.append_with_presentation(scene, transform, self.0.presentation);
    }

    /// Appends the retained local node with a transient presentation override.
    /// Geometry and vector content are unchanged, so interactive opacity previews
    /// do not trigger composition or node rebuilding.
    pub fn append_with_presentation(
        &self,
        scene: &mut Scene,
        transform: Option<Affine>,
        presentation: Presentation,
    ) {
        if !presentation.visible || !presentation.opacity.is_finite() || presentation.opacity <= 0.0
        {
            return;
        }
        let opacity = presentation.opacity.clamp(0.0, 1.0);
        let placement = transform.map_or(self.0.placement, |outer| outer * self.0.placement);
        if opacity < 1.0 {
            let local = self.0.node.local_bounds;
            scene.push_layer(
                Fill::NonZero,
                Mix::Normal,
                opacity,
                placement,
                &Rect::new(0.0, 0.0, f64::from(local.width), f64::from(local.height)),
            );
        }
        scene.append(&self.0.node.scene, Some(placement));
        if opacity < 1.0 {
            scene.pop_layer();
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Presentation {
    pub visible: bool,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayerKind {
    Pixel(PixelMetadata),
    Text(TextMetadata),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelMetadata {
    pub name: String,
    pub format: PixelFormat,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMetadata {
    pub text: String,
    pub language: Option<LanguageTag>,
    pub rendered_bounds: RenderBounds,
    pub layout_bounds: RenderBounds,
    pub post_script_fonts: Vec<String>,
    pub font_size: f32,
    pub color: [u8; 4],
    pub alignment: koharu_scene::TextAlignment,
    pub writing_mode: koharu_scene::WritingMode,
    pub angle_degrees: f32,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct CompositionStats {
    pub reused_layers: usize,
    pub built_layers: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderDiagnostic {
    UsedSourceText {
        entity: EntityId,
    },
    TextOverflow {
        entity: EntityId,
        available: RenderBounds,
        actual_width: f32,
        actual_height: f32,
        font_size: f32,
    },
    TextBelowReadableSize {
        entity: EntityId,
        font_size: f32,
        minimum_font_size: f32,
    },
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct RenderBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<LayoutBox> for RenderBounds {
    fn from(value: LayoutBox) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

#[derive(Clone)]
struct CompiledPage {
    revision: Revision,
    page: EntityId,
    width: u32,
    height: u32,
    layers: Vec<LayerDraft>,
    diagnostics: Vec<RenderDiagnostic>,
}

#[derive(Clone)]
struct LayerDraft {
    entity: EntityId,
    geometry: Geometry,
    frame: GeometryFrame,
    bounds: RenderBounds,
    presentation: Presentation,
    ancestry: Arc<[EntityId]>,
    content: LayerDraftContent,
}

impl LayerDraft {
    fn node_descriptor(&self) -> &NodeDescriptor {
        match &self.content {
            LayerDraftContent::Pixel { descriptor, .. } => descriptor,
            LayerDraftContent::Text { descriptor } => descriptor,
        }
    }
}

#[derive(Clone)]
enum LayerDraftContent {
    Pixel {
        name: String,
        format: PixelFormat,
        descriptor: NodeDescriptor,
    },
    Text {
        descriptor: NodeDescriptor,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum NodeDescriptor {
    Pixel(PixelNodeDescriptor),
    Text(Box<TextNodeDescriptor>),
}

#[derive(Clone, Debug, PartialEq)]
struct PixelNodeDescriptor {
    blob: BlobId,
    expected_size: Option<(u32, u32)>,
    format: PixelFormat,
}

struct RetainedNode {
    descriptor: NodeDescriptor,
    scene: Arc<Scene>,
    local_bounds: RenderBounds,
    text: Option<RenderedTextMetadata>,
    diagnostics: Arc<[RenderDiagnostic]>,
}

#[derive(Clone)]
struct RenderedTextMetadata {
    text: String,
    language: Option<LanguageTag>,
    rendered_bounds: RenderBounds,
    layout_bounds: RenderBounds,
    post_script_fonts: Vec<String>,
    font_size: f32,
    color: [u8; 4],
    alignment: koharu_scene::TextAlignment,
    writing_mode: koharu_scene::WritingMode,
}

fn compile_page(snapshot: &Snapshot, page_id: EntityId) -> Result<CompiledPage> {
    let page = snapshot.page(page_id)?.page()?;
    let (width, height) = surface_size(page.width, page.height)?;
    let mut layers = Vec::new();
    let mut diagnostics = Vec::new();
    let mut ancestry = Vec::new();
    visit_entity(
        snapshot,
        page_id,
        page_id,
        width,
        height,
        Presentation {
            visible: true,
            opacity: 1.0,
        },
        &mut ancestry,
        &mut layers,
        &mut diagnostics,
    )?;
    Ok(CompiledPage {
        revision: snapshot.revision(),
        page: page_id,
        width,
        height,
        layers,
        diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
fn visit_entity(
    snapshot: &Snapshot,
    entity: EntityId,
    page: EntityId,
    page_width: u32,
    page_height: u32,
    inherited: Presentation,
    ancestry: &mut Vec<EntityId>,
    output: &mut Vec<LayerDraft>,
    diagnostics: &mut Vec<RenderDiagnostic>,
) -> Result<()> {
    let own = snapshot.component::<Visibility>(entity)?.map_or(
        Presentation {
            visible: true,
            opacity: 1.0,
        },
        |visibility| Presentation {
            visible: visibility.visible,
            opacity: visibility.opacity,
        },
    );
    let presentation = Presentation {
        visible: inherited.visible && own.visible,
        opacity: inherited.opacity * own.opacity,
    };
    let pixel = snapshot.component::<PixelLayer>(entity)?;
    let text = snapshot.component::<AuthoredTextLayout>(entity)?;
    if pixel.is_some() && text.is_some() {
        return Err(Error::invalid(format!(
            "entity {entity} declares both pixel and text presentation"
        )));
    }
    if let Some(pixel) = pixel {
        let geometry = if entity == page {
            snapshot.component::<Geometry>(entity)?.unwrap_or_else(|| {
                Geometry::rectangle(0.0, 0.0, f64::from(page_width), f64::from(page_height))
            })
        } else {
            snapshot
                .component::<Geometry>(entity)?
                .ok_or_else(|| Error::invalid(format!("pixel layer {entity} has no geometry")))?
        };
        let asset = snapshot
            .asset(pixel.asset.owner, &pixel.asset.role)?
            .ok_or_else(|| Error::MissingAsset {
                entity,
                owner: pixel.asset.owner,
                role: pixel.asset.role.as_str().to_owned(),
            })?;
        let frame = valid_frame(entity, &geometry)?;
        let bounds = bubble::geometry_bounds(&geometry)
            .ok_or_else(|| Error::invalid(format!("pixel layer {entity} has invalid geometry")))?
            .into();
        let descriptor = NodeDescriptor::Pixel(PixelNodeDescriptor {
            blob: asset.blob,
            expected_size: asset.metadata.width.zip(asset.metadata.height),
            format: pixel.format,
        });
        output.push(LayerDraft {
            entity,
            geometry,
            frame,
            bounds,
            presentation,
            ancestry: ancestry.clone().into(),
            content: LayerDraftContent::Pixel {
                name: pixel.name,
                format: pixel.format,
                descriptor,
            },
        });
    } else if let Some(layout) = text {
        let layer = snapshot.text_layer(entity)?;
        let typography = layer.typography()?;
        let geometry = layer
            .frame()?
            .ok_or_else(|| Error::invalid(format!("text layer {entity} has no frame")))?;
        let frame = valid_frame(entity, &geometry)?;
        let content = layer.content()?;
        let translation = content.translation()?;
        let source = content.source()?;
        let (text, language) = if let Some(translation) = translation {
            (translation.text.value, translation.language)
        } else if let Some(source) = source {
            diagnostics.push(RenderDiagnostic::UsedSourceText { entity });
            (source.text.value, source.language)
        } else {
            return Err(Error::invalid(format!(
                "text layer {entity} content has neither source nor translation"
            )));
        };
        let balloon_contour = layer
            .fit_target()?
            .map(|region| -> Result<Option<Vec<(f32, f32)>>> {
                if region.region()?.kind == BubbleRegion::kind() {
                    Ok(Some(bubble::contour(&region.geometry()?, frame)))
                } else {
                    Ok(None)
                }
            })
            .transpose()?
            .flatten();
        let bounds = bubble::geometry_bounds(&geometry)
            .ok_or_else(|| Error::invalid(format!("text layer {entity} has invalid geometry")))?
            .into();
        let descriptor = NodeDescriptor::Text(Box::new(TextNodeDescriptor {
            entity,
            text,
            language,
            width: frame.bounds.width,
            height: frame.bounds.height,
            balloon_contour,
            layout,
            typography,
        }));
        output.push(LayerDraft {
            entity,
            geometry,
            frame,
            bounds,
            presentation,
            ancestry: ancestry.clone().into(),
            content: LayerDraftContent::Text { descriptor },
        });
    }

    let is_group = snapshot.component::<Group>(entity)?.is_some();
    if is_group {
        ancestry.push(entity);
    }
    for child in snapshot.children(entity)? {
        visit_entity(
            snapshot,
            child,
            page,
            page_width,
            page_height,
            presentation,
            ancestry,
            output,
            diagnostics,
        )?;
    }
    if is_group {
        ancestry.pop();
    }
    Ok(())
}

fn valid_frame(entity: EntityId, geometry: &Geometry) -> Result<GeometryFrame> {
    bubble::geometry_frame(geometry)
        .ok_or_else(|| Error::invalid(format!("layer {entity} has invalid geometry")))
}

fn surface_size(width: f64, height: f64) -> Result<(u32, u32)> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(Error::invalid(
            "page dimensions must be finite and positive",
        ));
    }
    let width = width.ceil() as u64;
    let height = height.ceil() as u64;
    if width > u64::from(MAX_SURFACE_DIMENSION)
        || height > u64::from(MAX_SURFACE_DIMENSION)
        || width.saturating_mul(height) > MAX_SURFACE_PIXELS
    {
        return Err(Error::invalid("page dimensions exceed renderer limits"));
    }
    Ok((width as u32, height as u32))
}

fn build_composition(
    compiled: CompiledPage,
    previous: HashMap<EntityId, Arc<RetainedNode>>,
    fonts: Arc<Fonts>,
    images: Arc<Mutex<DecodedImageCache>>,
    cache: Arc<Mutex<RetainedNodeCache>>,
) -> Result<Composition> {
    let mut layers = Vec::with_capacity(compiled.layers.len());
    let mut diagnostics = compiled.diagnostics;
    let mut stats = CompositionStats::default();
    for draft in compiled.layers {
        let descriptor = draft.node_descriptor().clone();
        let node = if let Some(node) = previous
            .get(&draft.entity)
            .filter(|node| node.descriptor == descriptor)
            .cloned()
            .or_else(|| cache.lock().get(compiled.page, draft.entity, &descriptor))
        {
            stats.reused_layers += 1;
            node
        } else {
            stats.built_layers += 1;
            let node = Arc::new(build_node(&descriptor, &fonts, &images)?);
            cache
                .lock()
                .insert(compiled.page, draft.entity, node.clone());
            node
        };
        diagnostics.extend(
            node.diagnostics
                .iter()
                .cloned()
                .map(|diagnostic| place_diagnostic(diagnostic, draft.frame)),
        );
        layers.push(layer_from_draft(draft, node));
    }
    let layers: Arc<[Layer]> = layers.into();
    let layer_index = layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (layer.entity(), index))
        .collect();
    Ok(Composition(Arc::new(CompositionData {
        revision: compiled.revision,
        page: compiled.page,
        width: compiled.width,
        height: compiled.height,
        origin: (0, 0),
        normalization: Affine::IDENTITY,
        layers,
        layer_index,
        diagnostics: diagnostics.into(),
        stats,
        scene: OnceLock::new(),
    })))
}

fn layer_from_draft(draft: LayerDraft, node: Arc<RetainedNode>) -> Layer {
    let placement = match &draft.content {
        LayerDraftContent::Pixel { .. } => pixel_frame_transform(draft.frame, node.local_bounds),
        LayerDraftContent::Text { .. } => frame_transform(draft.frame),
    };
    let kind = match draft.content {
        LayerDraftContent::Pixel { name, format, .. } => {
            LayerKind::Pixel(PixelMetadata { name, format })
        }
        LayerDraftContent::Text { .. } => {
            let text = node.text.as_ref().expect("text node metadata must exist");
            LayerKind::Text(TextMetadata {
                text: text.text.clone(),
                language: text.language.clone(),
                rendered_bounds: transform_bounds(text.rendered_bounds, placement),
                layout_bounds: transform_bounds(text.layout_bounds, placement),
                post_script_fonts: text.post_script_fonts.clone(),
                font_size: text.font_size,
                color: text.color,
                alignment: text.alignment,
                writing_mode: text.writing_mode,
                angle_degrees: draft.frame.angle_degrees,
            })
        }
    };
    Layer(Arc::new(LayerData {
        entity: draft.entity,
        geometry: draft.geometry,
        bounds: draft.bounds,
        presentation: draft.presentation,
        ancestry: draft.ancestry,
        kind,
        placement,
        node,
    }))
}

fn frame_transform(frame: GeometryFrame) -> Affine {
    let bounds = frame.bounds;
    let center = (
        f64::from(bounds.x + bounds.width * 0.5),
        f64::from(bounds.y + bounds.height * 0.5),
    );
    Affine::translate((f64::from(bounds.x), f64::from(bounds.y)))
        .then_rotate_about(f64::from(frame.angle_degrees).to_radians(), center)
}

fn pixel_frame_transform(frame: GeometryFrame, source: RenderBounds) -> Affine {
    let bounds = frame.bounds;
    let center = (
        f64::from(bounds.x + bounds.width * 0.5),
        f64::from(bounds.y + bounds.height * 0.5),
    );
    Affine::scale_non_uniform(
        f64::from(bounds.width / source.width.max(f32::EPSILON)),
        f64::from(bounds.height / source.height.max(f32::EPSILON)),
    )
    .then_translate((f64::from(bounds.x), f64::from(bounds.y)).into())
    .then_rotate_about(f64::from(frame.angle_degrees).to_radians(), center)
}

fn transform_bounds(bounds: RenderBounds, transform: Affine) -> RenderBounds {
    let rect = transform.transform_rect_bbox(Rect::new(
        f64::from(bounds.x),
        f64::from(bounds.y),
        f64::from(bounds.x + bounds.width),
        f64::from(bounds.y + bounds.height),
    ));
    RenderBounds {
        x: rect.x0 as f32,
        y: rect.y0 as f32,
        width: rect.width() as f32,
        height: rect.height() as f32,
    }
}

fn place_diagnostic(diagnostic: RenderDiagnostic, frame: GeometryFrame) -> RenderDiagnostic {
    match diagnostic {
        RenderDiagnostic::TextOverflow {
            entity,
            available,
            actual_width,
            actual_height,
            font_size,
        } => RenderDiagnostic::TextOverflow {
            entity,
            available: transform_bounds(available, frame_transform(frame)),
            actual_width,
            actual_height,
            font_size,
        },
        diagnostic => diagnostic,
    }
}

fn build_node(
    descriptor: &NodeDescriptor,
    fonts: &Fonts,
    images: &Mutex<DecodedImageCache>,
) -> Result<RetainedNode> {
    match descriptor {
        NodeDescriptor::Pixel(pixel) => build_pixel_node(pixel, images),
        NodeDescriptor::Text(text) => {
            let RenderedTextNode {
                scene,
                local_bounds,
                metadata,
                diagnostics,
            } = TextRenderer::new().render_descriptor(text, fonts)?;
            Ok(RetainedNode {
                descriptor: descriptor.clone(),
                scene,
                local_bounds,
                text: Some(RenderedTextMetadata {
                    text: text.text.clone(),
                    language: text.language.clone(),
                    rendered_bounds: metadata.rendered_bounds,
                    layout_bounds: metadata.layout_bounds,
                    post_script_fonts: metadata.post_script_fonts,
                    font_size: metadata.font_size,
                    color: metadata.color,
                    alignment: text.typography.alignment,
                    writing_mode: text.typography.writing_mode,
                }),
                diagnostics: diagnostics.into(),
            })
        }
    }
}

fn build_pixel_node(
    descriptor: &PixelNodeDescriptor,
    images: &Mutex<DecodedImageCache>,
) -> Result<RetainedNode> {
    let decoded = images
        .lock()
        .get(descriptor.blob)
        .ok_or_else(|| Error::invalid(format!("image {} was not loaded", descriptor.blob)))?;
    if descriptor
        .expected_size
        .is_some_and(|expected| expected != (decoded.width, decoded.height))
    {
        return Err(Error::invalid(format!(
            "blob {} decoded as {}x{}, expected {:?}",
            descriptor.blob, decoded.width, decoded.height, descriptor.expected_size
        )));
    }
    let pixels = match descriptor.format {
        PixelFormat::Color => decoded.pixels.clone(),
        PixelFormat::Mask { channel, tint } => {
            let mut output = Vec::with_capacity(decoded.pixels.len());
            for pixel in decoded.pixels.chunks_exact(4) {
                let mask = match channel {
                    koharu_scene::MaskChannel::Luminance => (0.2126 * f32::from(pixel[0])
                        + 0.7152 * f32::from(pixel[1])
                        + 0.0722 * f32::from(pixel[2]))
                    .round() as u8,
                    koharu_scene::MaskChannel::Red => pixel[0],
                    koharu_scene::MaskChannel::Green => pixel[1],
                    koharu_scene::MaskChannel::Blue => pixel[2],
                    koharu_scene::MaskChannel::Alpha => pixel[3],
                };
                output.extend_from_slice(&[
                    tint[0],
                    tint[1],
                    tint[2],
                    (u16::from(mask) * u16::from(tint[3]) / 255) as u8,
                ]);
            }
            Arc::from(output)
        }
    };
    let bytes: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(ImageBytes(pixels));
    let data = ImageData {
        data: PenikoBlob::new(bytes),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: decoded.width,
        height: decoded.height,
    };
    let mut scene = Scene::new();
    scene.draw_image(&data, Affine::IDENTITY);
    Ok(RetainedNode {
        descriptor: NodeDescriptor::Pixel(descriptor.clone()),
        scene: Arc::new(scene),
        local_bounds: RenderBounds {
            x: 0.0,
            y: 0.0,
            width: decoded.width as f32,
            height: decoded.height as f32,
        },
        text: None,
        diagnostics: Arc::from([]),
    })
}

fn decode_image(
    blob: BlobId,
    bytes: &Arc<[u8]>,
    expected: Option<(u32, u32)>,
) -> Result<(BlobId, Arc<DecodedImage>)> {
    let decoded = image::load_from_memory(bytes)
        .map_err(|source| Error::Image { blob, source })?
        .into_rgba8();
    if expected.is_some_and(|size| size != decoded.dimensions()) {
        return Err(Error::invalid(format!(
            "blob {blob} decoded as {}x{}, expected {:?}",
            decoded.width(),
            decoded.height(),
            expected
        )));
    }
    Ok((
        blob,
        Arc::new(DecodedImage {
            width: decoded.width(),
            height: decoded.height(),
            pixels: Arc::from(decoded.into_raw()),
        }),
    ))
}

fn draw_font_preview(scene: &mut Scene, layout: &crate::LayoutRun<'_>) -> anyhow::Result<()> {
    let brush = rgba([0, 0, 0, 255]);
    for line in &layout.lines {
        let (baseline_x, baseline_y) = line.baseline;
        let mut pen_x = 0.0;
        let mut pen_y = 0.0;
        for glyph in &line.glyphs {
            let font = glyph.font.skrifa_ref()?;
            if let Some(outline) = font.outline_glyphs().get(GlyphId::new(glyph.glyph_id)) {
                let mut path = BezPath::new();
                outline.draw(
                    DrawSettings::unhinted(Size::new(layout.font_size), glyph.font.location()),
                    &mut PreviewOutline(&mut path),
                )?;
                let transform = Affine::translate((
                    f64::from(baseline_x + pen_x + glyph.x_offset),
                    f64::from(baseline_y + pen_y - glyph.y_offset),
                )) * Affine::scale_non_uniform(1.0, -1.0);
                scene.fill(Fill::NonZero, transform, brush, None, &path);
            }
            pen_x += glyph.x_advance;
            pen_y -= glyph.y_advance;
        }
    }
    Ok(())
}

struct PreviewOutline<'a>(&'a mut BezPath);

impl OutlinePen for PreviewOutline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), f64::from(y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), f64::from(y)));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(
            (f64::from(cx0), f64::from(cy0)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to(
            (f64::from(cx0), f64::from(cy0)),
            (f64::from(cx1), f64::from(cy1)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn close(&mut self) {
        self.0.close_path();
    }
}

struct ImageBytes(Arc<[u8]>);

impl AsRef<[u8]> for ImageBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
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

    fn contains(&self, blob: BlobId) -> bool {
        self.entries.contains_key(&blob)
    }

    fn get(&mut self, blob: BlobId) -> Option<Arc<DecodedImage>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(&blob)?;
        entry.last_used = self.clock;
        Some(entry.image.clone())
    }

    fn insert(&mut self, blob: BlobId, image: Arc<DecodedImage>) {
        self.clock = self.clock.wrapping_add(1);
        if let Some(previous) = self.entries.remove(&blob) {
            self.bytes = self.bytes.saturating_sub(previous.image.byte_len());
        }
        self.bytes = self.bytes.saturating_add(image.byte_len());
        self.entries.insert(
            blob,
            CachedImage {
                image,
                last_used: self.clock,
            },
        );
        while self.bytes > self.max_bytes && self.entries.len() > 1 {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(blob, _)| *blob)
            else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(removed.image.byte_len());
            }
        }
    }
}

struct CachedNode {
    node: Arc<RetainedNode>,
    last_used: u64,
}

struct RetainedNodeCache {
    entries: HashMap<(EntityId, EntityId), CachedNode>,
    capacity: usize,
    clock: u64,
}

impl RetainedNodeCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
            clock: 0,
        }
    }

    fn contains(&self, page: EntityId, entity: EntityId, descriptor: &NodeDescriptor) -> bool {
        self.entries
            .get(&(page, entity))
            .is_some_and(|entry| entry.node.descriptor == *descriptor)
    }

    fn get(
        &mut self,
        page: EntityId,
        entity: EntityId,
        descriptor: &NodeDescriptor,
    ) -> Option<Arc<RetainedNode>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(&(page, entity))?;
        if entry.node.descriptor != *descriptor {
            return None;
        }
        entry.last_used = self.clock;
        Some(entry.node.clone())
    }

    fn insert(&mut self, page: EntityId, entity: EntityId, node: Arc<RetainedNode>) {
        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(
            (page, entity),
            CachedNode {
                node,
                last_used: self.clock,
            },
        );
        if self.entries.len() > self.capacity
            && let Some((oldest, _)) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, entry)| (*key, entry.last_used))
        {
            self.entries.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, io::Cursor};

    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use koharu_scene::{
        AssetInput, AssetMetadata, AssetRef, AssetRole, At, Geometry, MaskChannel, Origin,
        PageDraft, PixelLayer, Session, Visibility,
    };

    use super::*;

    fn png(color: [u8; 4]) -> Arc<[u8]> {
        let image = RgbaImage::from_pixel(2, 2, Rgba(color));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        Arc::from(bytes.into_inner())
    }

    fn asset(bytes: Arc<[u8]>) -> AssetInput {
        AssetInput::new(
            bytes,
            "image/png",
            AssetMetadata {
                width: Some(2),
                height: Some(2),
                attributes: BTreeMap::new(),
            },
        )
    }

    #[tokio::test]
    async fn explicit_layers_retain_nodes_and_ignore_analysis_only_entities() {
        let mut session = Session::memory().unwrap();
        let original = AssetRole::new("original").unwrap();
        let overlay = AssetRole::new("overlay").unwrap();
        let mut ids = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("Page", 100.0, 80.0), At::End)?;
                edit.set_asset(page, &original, asset(png([255, 255, 255, 255])))?;
                edit.set(
                    page,
                    &PixelLayer::color(
                        Origin::User,
                        "Original",
                        AssetRef::new(page, original.clone()),
                    ),
                )?;
                let analysis = edit.add_entity(page, At::End)?;
                edit.set(analysis, &Geometry::rectangle(4.0, 5.0, 6.0, 7.0))?;
                let visual = edit.add_entity(page, At::End)?;
                edit.set_asset(visual, &overlay, asset(png([255, 0, 0, 255])))?;
                edit.set(visual, &Geometry::rectangle(10.0, 20.0, 30.0, 40.0))?;
                edit.set(
                    visual,
                    &PixelLayer::color(
                        Origin::User,
                        "Overlay",
                        AssetRef::new(visual, overlay.clone()),
                    ),
                )?;
                ids = Some((page, analysis, visual));
                Ok(())
            })
            .unwrap();
        let initial = session.commit(patch).unwrap();
        let (page, analysis, visual) = ids.unwrap();
        let renderer = Renderer::new().unwrap();
        let first = renderer.compose(&initial.snapshot, page).await.unwrap();
        assert_eq!(first.layers().len(), 2);
        assert!(first.layer(analysis).is_none());
        assert!(first.layer(visual).is_some());
        assert_eq!(first.stats().built_layers, 2);

        let patch = session
            .snapshot()
            .patch(|edit| edit.set(visual, &Geometry::rectangle(12.0, 22.0, 30.0, 40.0)))
            .unwrap();
        let moved = session.commit(patch).unwrap();
        let second = renderer
            .update(&first, &moved.snapshot, &moved.changes)
            .await
            .unwrap();
        assert_eq!(second.stats().reused_layers, 2);
        assert_eq!(second.stats().built_layers, 0);

        let patch = session
            .snapshot()
            .patch(|edit| {
                edit.set(
                    visual,
                    &PixelLayer::mask(
                        Origin::User,
                        "Overlay Mask",
                        AssetRef::new(visual, overlay.clone()),
                        MaskChannel::Alpha,
                        [0, 0, 255, 255],
                    ),
                )
            })
            .unwrap();
        let masked = session.commit(patch).unwrap();
        let third = renderer
            .update(&second, &masked.snapshot, &masked.changes)
            .await
            .unwrap();
        assert_eq!(third.stats().reused_layers, 1);
        assert_eq!(third.stats().built_layers, 1);
    }

    #[tokio::test]
    async fn cropped_composition_extracts_hidden_layer_at_full_presentation() {
        let mut session = Session::memory().unwrap();
        let role = AssetRole::new("layer").unwrap();
        let mut ids = None;
        let patch = session
            .snapshot()
            .patch(|edit| {
                let page = edit.add_page(PageDraft::new("Page", 64.0, 64.0), At::End)?;
                let layer = edit.add_entity(page, At::End)?;
                edit.set_asset(layer, &role, asset(png([10, 20, 30, 255])))?;
                edit.set(layer, &Geometry::rectangle(10.25, 20.5, 16.0, 12.0))?;
                edit.set(
                    layer,
                    &PixelLayer::color(Origin::User, "Hidden", AssetRef::new(layer, role.clone())),
                )?;
                edit.set(
                    layer,
                    &Visibility {
                        origin: Origin::User,
                        visible: false,
                        opacity: 0.25,
                    },
                )?;
                ids = Some((page, layer));
                Ok(())
            })
            .unwrap();
        let commit = session.commit(patch).unwrap();
        let (page, layer) = ids.unwrap();
        let renderer = Renderer::new().unwrap();
        let composition = renderer.compose(&commit.snapshot, page).await.unwrap();
        assert!(!composition.layer(layer).unwrap().presentation().visible);
        let cropped = composition.cropped(layer).unwrap().unwrap();
        assert_eq!(cropped.origin(), (10, 20));
        assert_eq!(cropped.size(), (17, 13));
        assert_eq!(
            cropped.layer(layer).unwrap().presentation(),
            Presentation {
                visible: true,
                opacity: 1.0,
            }
        );
    }
}
