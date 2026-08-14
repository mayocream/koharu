//! Immutable retained page output and synchronous vector access.

use std::{collections::HashMap, sync::Arc};

use koharu_scene::{BlobId, EntityId, Geometry, LanguageTag, RelationId, Revision};
use vello::{
    Scene,
    kurbo::{Affine, Rect},
    peniko::{Fill, Mix},
};

use crate::{Error, Result, TextAlign, WritingMode};

const MAX_SURFACE_DIMENSION: u32 = 32_768;
const MAX_SURFACE_PIXELS: u64 = 268_435_456;

#[derive(Clone)]
pub struct Frame(pub(crate) Arc<FrameData>);

pub(crate) struct FrameData {
    pub(crate) revision: Revision,
    pub(crate) page: EntityId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) origin: (i32, i32),
    pub(crate) normalization: Affine,
    pub(crate) layers: Arc<[Layer]>,
    pub(crate) layer_index: Arc<HashMap<EntityId, usize>>,
    pub(crate) dependencies: Arc<[RenderDependency]>,
    pub(crate) diagnostics: Arc<[RenderDiagnostic]>,
    pub(crate) stats: RetentionStats,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Frame")
            .field("revision", &self.revision())
            .field("page", &self.page())
            .field("size", &self.size())
            .field("origin", &self.origin())
            .field("layers", &self.layers())
            .field("diagnostics", &self.diagnostics())
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl Frame {
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
    pub fn dependencies(&self) -> &[RenderDependency] {
        &self.0.dependencies
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[RenderDiagnostic] {
        &self.0.diagnostics
    }

    #[must_use]
    pub fn stats(&self) -> RetentionStats {
        self.0.stats
    }

    /// Returns one entity normalized into a tightly cropped frame.
    ///
    /// The original layer retains authored presentation. The isolated copy is
    /// visible at full opacity so layered exports can store pixels separately
    /// from their visibility and opacity metadata.
    pub fn cropped(&self, entity: EntityId) -> Result<Option<Self>> {
        let Some(layer) = self.layer(entity).cloned() else {
            return Ok(None);
        };
        let bounds = layer.bounds();
        if ![bounds.x, bounds.y, bounds.width, bounds.height]
            .into_iter()
            .all(f32::is_finite)
            || bounds.width < 0.0
            || bounds.height < 0.0
        {
            return Err(Error::invalid(format!(
                "visual layer bounds are not finite for entity {entity}"
            )));
        }
        let edges = [
            bounds.x.floor(),
            bounds.y.floor(),
            (bounds.x + bounds.width).ceil(),
            (bounds.y + bounds.height).ceil(),
        ];
        if edges.iter().any(|edge| {
            f64::from(*edge) < f64::from(i32::MIN) || f64::from(*edge) > f64::from(i32::MAX)
        }) {
            return Err(Error::invalid("visual layer crop origin exceeds i32"));
        }
        let [left, top, right, bottom] = edges.map(|edge| edge as i32);
        let width = u32::try_from((i64::from(right) - i64::from(left)).max(1))
            .map_err(|_| Error::invalid("visual layer width exceeds u32"))?;
        let height = u32::try_from((i64::from(bottom) - i64::from(top)).max(1))
            .map_err(|_| Error::invalid("visual layer height exceeds u32"))?;
        if width > MAX_SURFACE_DIMENSION
            || height > MAX_SURFACE_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_SURFACE_PIXELS
        {
            return Err(Error::invalid(format!(
                "cropped surface {width}x{height} exceeds renderer limits"
            )));
        }
        let isolated = Layer(Arc::new(LayerData {
            presentation: Presentation {
                visible: true,
                opacity: 1.0,
            },
            ..layer.0.clone_for_frame()
        }));
        let layers: Arc<[Layer]> = vec![isolated].into();
        Ok(Some(Self(Arc::new(FrameData {
            revision: self.revision(),
            page: self.page(),
            width,
            height,
            origin: (left, top),
            normalization: Affine::translate((-f64::from(left), -f64::from(top))),
            layers,
            layer_index: Arc::new(HashMap::from([(entity, 0)])),
            dependencies: self.0.dependencies.clone(),
            diagnostics: self.0.diagnostics.clone(),
            stats: self.0.stats,
        }))))
    }

    pub(crate) fn at_revision(&self, revision: Revision, stats: RetentionStats) -> Self {
        Self(Arc::new(FrameData {
            revision,
            page: self.0.page,
            width: self.0.width,
            height: self.0.height,
            origin: self.0.origin,
            normalization: self.0.normalization,
            layers: self.0.layers.clone(),
            layer_index: self.0.layer_index.clone(),
            dependencies: self.0.dependencies.clone(),
            diagnostics: self.0.diagnostics.clone(),
            stats,
        }))
    }
}

#[derive(Clone)]
pub struct Layer(pub(crate) Arc<LayerData>);

impl std::fmt::Debug for Layer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Layer")
            .field("entity", &self.entity())
            .field("bounds", &self.bounds())
            .field("presentation", &self.presentation())
            .field("kind", self.kind())
            .finish_non_exhaustive()
    }
}

pub(crate) struct LayerData {
    pub(crate) entity: EntityId,
    pub(crate) geometry: Geometry,
    pub(crate) bounds: RenderBounds,
    pub(crate) presentation: Presentation,
    pub(crate) ancestry: Arc<[EntityId]>,
    pub(crate) kind: LayerKind,
    pub(crate) placement: Affine,
    pub(crate) node: Arc<RetainedNode>,
    pub(crate) dependencies: Arc<[RenderDependency]>,
}

impl LayerData {
    fn clone_for_frame(&self) -> Self {
        Self {
            entity: self.entity,
            geometry: self.geometry.clone(),
            bounds: self.bounds,
            presentation: self.presentation,
            ancestry: self.ancestry.clone(),
            kind: self.kind.clone(),
            placement: self.placement,
            node: self.node.clone(),
            dependencies: self.dependencies.clone(),
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

    #[must_use]
    pub fn dependencies(&self) -> &[RenderDependency] {
        &self.0.dependencies
    }

    #[must_use]
    pub fn raster_image(&self) -> Option<&RasterImage> {
        self.0.node.image.as_ref()
    }

    #[must_use]
    pub fn placement(&self) -> Affine {
        self.0.placement
    }

    pub fn append_vector_with_presentation(
        &self,
        scene: &mut Scene,
        transform: Option<Affine>,
        presentation: Presentation,
    ) {
        if self.raster_image().is_some() {
            return;
        }
        if !presentation.visible || !presentation.opacity.is_finite() || presentation.opacity <= 0.0
        {
            return;
        }
        let opacity = presentation.opacity.clamp(0.0, 1.0);
        let placement = transform.map_or(self.0.placement, |outer| outer * self.0.placement);
        if opacity < 1.0 {
            let bounds = self.0.node.local_bounds;
            scene.push_layer(
                Fill::NonZero,
                Mix::Normal,
                opacity,
                placement,
                &Rect::new(
                    f64::from(bounds.x),
                    f64::from(bounds.y),
                    f64::from(bounds.x + bounds.width),
                    f64::from(bounds.y + bounds.height),
                ),
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
    Image(ImageMetadata),
    Text(TextMetadata),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ImageKind {
    Source,
    Cleanup,
    Paint,
    Embedded,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageMetadata {
    pub name: Option<String>,
    pub kind: ImageKind,
}

#[derive(Clone)]
pub struct RasterImage {
    pub(crate) blob: BlobId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Arc<[u8]>,
}

impl RasterImage {
    #[must_use]
    pub fn blob(&self) -> BlobId {
        self.blob
    }

    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
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
    pub alignment: TextAlign,
    pub writing_mode: WritingMode,
    pub angle_degrees: f32,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct RetentionStats {
    pub candidate_layers: usize,
    pub reused_layers: usize,
    pub rebuilt_layers: usize,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RenderDependency {
    Entity(EntityId),
    Hierarchy(EntityId),
    Component { entity: EntityId, kind: String },
    Relation(RelationId),
    RelationQuery { source: EntityId, kind: String },
    RelationTargetQuery { target: EntityId, kind: String },
    Blob(BlobId),
    Font(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderDiagnostic {
    MissingAsset {
        entity: EntityId,
        role: String,
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

impl From<crate::bubble::LayoutBox> for RenderBounds {
    fn from(value: crate::bubble::LayoutBox) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NodeDescriptor {
    Image(ImageNodeDescriptor),
    Text(Box<crate::text_renderer::TextNodeDescriptor>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ImageNodeDescriptor {
    pub(crate) blob: BlobId,
    pub(crate) expected_size: Option<(u32, u32)>,
    pub(crate) require_size: Option<(u32, u32)>,
}

pub(crate) struct RetainedNode {
    pub(crate) descriptor: NodeDescriptor,
    pub(crate) scene: Arc<Scene>,
    pub(crate) local_bounds: RenderBounds,
    pub(crate) image: Option<RasterImage>,
    pub(crate) text: Option<LocalTextMetadata>,
    pub(crate) diagnostics: Arc<[RenderDiagnostic]>,
}

#[derive(Clone)]
pub(crate) struct LocalTextMetadata {
    pub(crate) rendered_bounds: RenderBounds,
    pub(crate) layout_bounds: RenderBounds,
    pub(crate) post_script_fonts: Vec<String>,
    pub(crate) font_size: f32,
    pub(crate) color: [u8; 4],
}
