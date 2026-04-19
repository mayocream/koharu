//! Scene graph: Project → Pages → flat Nodes.
//!
//! Three primitives: `Node`, `Blob` (via `BlobRef`), `Op` (in `op.rs`).
//! Everything visual on a page is a `Node`; scene mutations flow through `Op`s.

// `NodeKind::Text` naturally carries more data than `Image`/`Mask`, and
// boxing would change the wire format. Same reasoning as in `op.rs`.
#![allow(clippy::large_enum_variant)]

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::blob::BlobRef;
use crate::font::{FontPrediction, TextDirection};
use crate::style::TextStyle;

// ---------------------------------------------------------------------------
// Ids
// ---------------------------------------------------------------------------

#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    JsonSchema,
    ToSchema,
)]
#[serde(transparent)]
pub struct PageId(pub Uuid);

#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    JsonSchema,
    ToSchema,
)]
#[serde(transparent)]
pub struct NodeId(pub Uuid);

impl PageId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl NodeId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for PageId {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for PageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ---------------------------------------------------------------------------
// Scene / Project
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub project: ProjectMeta,
    /// Pages in insertion order; `IndexMap` ordering *is* the page order.
    pub pages: IndexMap<PageId, Page>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            project: ProjectMeta::default(),
            pages: IndexMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub style: ProjectStyle,
}

impl Default for ProjectMeta {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            name: String::new(),
            created_at: now,
            updated_at: now,
            style: ProjectStyle::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStyle {
    #[serde(default)]
    pub default_font: Option<String>,
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: PageId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Stacking = insertion order. Bottom-first: `source` is typically first,
    /// `rendered` typically last.
    pub nodes: IndexMap<NodeId, Node>,
}

impl Page {
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            id: PageId::new(),
            name: name.into(),
            width,
            height,
            nodes: IndexMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Node
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: NodeId,
    #[serde(default)]
    pub transform: Transform,
    pub visible: bool,
    pub kind: NodeKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    Image(ImageData),
    Text(TextData),
    Mask(MaskData),
}

impl NodeKind {
    pub fn discriminant(&self) -> NodeKindTag {
        match self {
            NodeKind::Image(_) => NodeKindTag::Image,
            NodeKind::Text(_) => NodeKindTag::Text,
            NodeKind::Mask(_) => NodeKindTag::Mask,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NodeKindTag {
    Image,
    Text,
    Mask,
}

// ---------------------------------------------------------------------------
// Image node
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageData {
    /// Role tags differentiate source / inpainted / rendered / user-imported images.
    /// Role is immutable on an existing node — switching roles = delete + add.
    pub role: ImageRole,
    pub blob: BlobRef,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    pub natural_width: u32,
    pub natural_height: u32,
    #[serde(default)]
    pub name: Option<String>,
}

const fn default_opacity() -> f32 {
    1.0
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ImageRole {
    /// Immutable page input; exactly one per page.
    Source,
    /// Pipeline output; text removed from `Source`.
    Inpainted,
    /// Pipeline output; final composite.
    Rendered,
    /// User-imported free layer, movable / selectable.
    Custom,
}

// ---------------------------------------------------------------------------
// Mask node
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MaskData {
    pub role: MaskRole,
    pub blob: BlobRef,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum MaskRole {
    /// Manual brush strokes driving local inpaint.
    BrushInpaint,
    /// Text-detector segmentation preview (text-pixel mask).
    Segment,
    /// Bubble-interior mask from `speech-bubble-segmentation`. The
    /// renderer grows text layout boxes inside this mask so English
    /// wraps into the available bubble space without leaking past the
    /// bubble border.
    Bubble,
}

// ---------------------------------------------------------------------------
// Text node
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextData {
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub source_lang: Option<String>,
    #[serde(default)]
    pub source_direction: Option<TextDirection>,
    #[serde(default)]
    pub rendered_direction: Option<TextDirection>,
    #[serde(default)]
    pub line_polygons: Option<Vec<[[f32; 2]; 4]>>,
    #[serde(default)]
    pub rotation_deg: Option<f32>,
    #[serde(default)]
    pub detected_font_size_px: Option<f32>,
    #[serde(default)]
    pub detector: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub translation: Option<String>,
    #[serde(default)]
    pub style: Option<TextStyle>,
    #[serde(default)]
    pub font_prediction: Option<FontPrediction>,
    /// Renderer-produced sprite for this block.
    #[serde(default)]
    pub sprite: Option<BlobRef>,
    /// Sprite placement when the renderer expands past the bubble geometry.
    #[serde(default)]
    pub sprite_transform: Option<Transform>,
    #[serde(default)]
    pub lock_layout_box: bool,
}

// ---------------------------------------------------------------------------
// Transform
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub rotation_deg: f32,
}

// ---------------------------------------------------------------------------
// Scene convenience helpers
// ---------------------------------------------------------------------------

impl Scene {
    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.get(&id)
    }

    pub fn page_mut(&mut self, id: PageId) -> Option<&mut Page> {
        self.pages.get_mut(&id)
    }

    pub fn node(&self, page: PageId, node: NodeId) -> Option<&Node> {
        self.page(page)?.nodes.get(&node)
    }

    pub fn node_mut(&mut self, page: PageId, node: NodeId) -> Option<&mut Node> {
        self.page_mut(page)?.nodes.get_mut(&node)
    }
}

impl Page {
    pub fn source_node(&self) -> Option<(&NodeId, &Node)> {
        self.nodes.iter().find(|(_, node)| {
            matches!(
                &node.kind,
                NodeKind::Image(img) if img.role == ImageRole::Source
            )
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_datetime_postcard_round_trips() {
        let now: DateTime<Utc> = Utc::now();
        let bytes = postcard::to_allocvec(&now).expect("serialize");
        let decoded: DateTime<Utc> = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(decoded.timestamp(), now.timestamp());
    }

    #[test]
    fn project_style_postcard_round_trips() {
        let style = ProjectStyle::default();
        let bytes = postcard::to_allocvec(&style).expect("serialize");
        let _: ProjectStyle = postcard::from_bytes(&bytes).expect("deserialize");
    }

    #[test]
    fn project_meta_postcard_round_trips() {
        let meta = ProjectMeta::default();
        let bytes = postcard::to_allocvec(&meta).expect("serialize");
        let decoded: ProjectMeta = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(decoded.name, meta.name);
    }

    #[test]
    fn empty_scene_postcard_round_trips() {
        let scene = Scene::default();
        let bytes = postcard::to_allocvec(&scene).expect("serialize");
        let decoded: Scene = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(decoded.pages.len(), 0);
    }

    #[test]
    fn scene_with_one_page_postcard_round_trips() {
        let mut scene = Scene::default();
        scene.project.name = "hello".into();
        let page = Page::new("p1", 800, 600);
        let page_id = page.id;
        scene.pages.insert(page_id, page);
        let bytes = postcard::to_allocvec(&scene).expect("serialize");
        let decoded: Scene = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(decoded.pages.len(), 1);
        assert_eq!(decoded.project.name, "hello");
        assert!(decoded.pages.contains_key(&page_id));
    }
}
