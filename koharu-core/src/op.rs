//! Scene mutations. Every scene change is an `Op`.
//!
//! Each `Op` carries the inverse payload (the `prev_*` / `prev` fields) so
//! `inverse()` is pure and local — no scene scan needed.
//!
//! The intended flow:
//! ```ignore
//! let mut op = Op::update_node(&scene, page, node, patch)?;   // reads prev
//! op.apply(&mut scene)?;                                      // mutates
//! let undo = op.inverse();                                    // pure
//! ```

// `Op` / `NodeDataPatch` are wire-format data types; their variant-size
// asymmetry is inherent (Text patches carry many optional fields), and
// boxing would change the serialised representation. Silence clippy here
// rather than smear `#[allow]` across every producer.
#![allow(clippy::large_enum_variant)]

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

use crate::blob::BlobRef;
use crate::font::{FontPrediction, TextDirection};
use crate::scene::{
    ImageData, ImageRole, MaskData, MaskRole, Node, NodeId, NodeKind, NodeKindTag, Page, PageId,
    ProjectStyle, Scene, TextData, Transform,
};
use crate::style::TextStyle;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum OpError {
    #[error("page not found: {0}")]
    PageNotFound(PageId),
    #[error("node not found in page {page}: {node}")]
    NodeNotFound { page: PageId, node: NodeId },
    #[error("page already exists: {0}")]
    PageExists(PageId),
    #[error("node already exists: {0}")]
    NodeExists(NodeId),
    #[error("insert index {index} out of range (len {len})")]
    IndexOutOfRange { index: usize, len: usize },
    #[error("reorder set differs from page/node set")]
    ReorderSetMismatch,
    #[error(
        "node kind mismatch: patch is {patch:?} but existing node is {existing:?} — delete + add instead"
    )]
    NodeKindMismatch {
        patch: NodeKindTag,
        existing: NodeKindTag,
    },
    #[error("scene invariant violated: {0}")]
    Invariant(&'static str),
}

pub type OpResult<T = ()> = Result<T, OpError>;

// ---------------------------------------------------------------------------
// Op
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Op {
    // Project
    UpdateProjectMeta {
        patch: ProjectMetaPatch,
        #[serde(default)]
        prev: ProjectMetaPatch,
    },

    // Pages
    AddPage {
        page: Page,
        at: usize,
    },
    RemovePage {
        id: PageId,
        prev_page: Page,
        prev_index: usize,
    },
    UpdatePage {
        id: PageId,
        patch: PagePatch,
        #[serde(default)]
        prev: PagePatch,
    },
    ReorderPages {
        order: Vec<PageId>,
        prev_order: Vec<PageId>,
    },

    // Nodes
    AddNode {
        page: PageId,
        node: Node,
        at: usize,
    },
    RemoveNode {
        page: PageId,
        id: NodeId,
        prev_node: Node,
        prev_index: usize,
    },
    UpdateNode {
        page: PageId,
        id: NodeId,
        patch: NodePatch,
        #[serde(default)]
        prev: NodePatch,
    },
    ReorderNodes {
        page: PageId,
        order: Vec<NodeId>,
        prev_order: Vec<NodeId>,
    },

    // Batch — one user action, many mutations, one history entry.
    Batch {
        #[schema(no_recursion)]
        ops: Vec<Op>,
        label: String,
    },
}

// ---------------------------------------------------------------------------
// Patches — sparse `Option<T>` values; `Option<Option<T>>` for clearable fields.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetaPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub style: Option<ProjectStyle>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PagePatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodePatch {
    #[serde(default)]
    pub transform: Option<Transform>,
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub data: Option<NodeDataPatch>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeDataPatch {
    Text(TextDataPatch),
    Image(ImageDataPatch),
    Mask(MaskDataPatch),
}

impl NodeDataPatch {
    pub fn tag(&self) -> NodeKindTag {
        match self {
            NodeDataPatch::Text(_) => NodeKindTag::Text,
            NodeDataPatch::Image(_) => NodeKindTag::Image,
            NodeDataPatch::Mask(_) => NodeKindTag::Mask,
        }
    }
}

/// For fields where "set to None" is meaningful (e.g. clearing a translation),
/// the outer `Option` is "patch present", the inner is "value present".
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextDataPatch {
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub source_lang: Option<Option<String>>,
    #[serde(default)]
    pub source_direction: Option<Option<TextDirection>>,
    #[serde(default)]
    pub rendered_direction: Option<Option<TextDirection>>,
    #[serde(default)]
    pub line_polygons: Option<Option<Vec<[[f32; 2]; 4]>>>,
    #[serde(default)]
    pub rotation_deg: Option<Option<f32>>,
    #[serde(default)]
    pub detected_font_size_px: Option<Option<f32>>,
    #[serde(default)]
    pub detector: Option<Option<String>>,
    #[serde(default)]
    pub text: Option<Option<String>>,
    #[serde(default)]
    pub translation: Option<Option<String>>,
    #[serde(default)]
    pub style: Option<Option<TextStyle>>,
    #[serde(default)]
    pub font_prediction: Option<Option<FontPrediction>>,
    #[serde(default)]
    pub sprite: Option<Option<BlobRef>>,
    #[serde(default)]
    pub sprite_transform: Option<Option<Transform>>,
    #[serde(default)]
    pub lock_layout_box: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageDataPatch {
    // `role` is immutable — to change, delete + add.
    #[serde(default)]
    pub blob: Option<BlobRef>,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default)]
    pub name: Option<Option<String>>,
    #[serde(default)]
    pub natural_width: Option<u32>,
    #[serde(default)]
    pub natural_height: Option<u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MaskDataPatch {
    #[serde(default)]
    pub blob: Option<BlobRef>,
}

// ---------------------------------------------------------------------------
// Op::apply
// ---------------------------------------------------------------------------

impl Op {
    /// Validate against the current scene, fill any empty `prev` fields with
    /// the state that will be overwritten, then mutate. Safe to call on a
    /// freshly-constructed Op; subsequent calls would overwrite `prev` again,
    /// so apply each Op only once.
    pub fn apply(&mut self, scene: &mut Scene) -> OpResult {
        match self {
            Op::UpdateProjectMeta { patch, prev } => {
                *prev = ProjectMetaPatch {
                    name: patch.name.as_ref().map(|_| scene.project.name.clone()),
                    style: patch.style.as_ref().map(|_| scene.project.style.clone()),
                    updated_at: patch.updated_at.as_ref().map(|_| scene.project.updated_at),
                };
                if let Some(name) = &patch.name {
                    scene.project.name = name.clone();
                }
                if let Some(style) = &patch.style {
                    scene.project.style = style.clone();
                }
                if let Some(ts) = patch.updated_at {
                    scene.project.updated_at = ts;
                }
            }

            Op::AddPage { page, at } => {
                if scene.pages.contains_key(&page.id) {
                    return Err(OpError::PageExists(page.id));
                }
                let len = scene.pages.len();
                if *at > len {
                    return Err(OpError::IndexOutOfRange { index: *at, len });
                }
                // IndexMap has no insert-at; insert, then shift into place.
                scene.pages.insert(page.id, page.clone());
                let last = scene.pages.len() - 1;
                if *at < last {
                    scene.pages.move_index(last, *at);
                }
            }

            Op::RemovePage {
                id,
                prev_page,
                prev_index,
            } => {
                let index = scene
                    .pages
                    .get_index_of(id)
                    .ok_or(OpError::PageNotFound(*id))?;
                let (_, page) = scene
                    .pages
                    .shift_remove_index(index)
                    .ok_or(OpError::PageNotFound(*id))?;
                *prev_page = page;
                *prev_index = index;
            }

            Op::UpdatePage { id, patch, prev } => {
                let page = scene.page_mut(*id).ok_or(OpError::PageNotFound(*id))?;
                *prev = PagePatch {
                    name: patch.name.as_ref().map(|_| page.name.clone()),
                    width: patch.width.as_ref().map(|_| page.width),
                    height: patch.height.as_ref().map(|_| page.height),
                };
                if let Some(name) = &patch.name {
                    page.name = name.clone();
                }
                if let Some(w) = patch.width {
                    page.width = w;
                }
                if let Some(h) = patch.height {
                    page.height = h;
                }
            }

            Op::ReorderPages { order, prev_order } => {
                ensure_same_page_set(&scene.pages, order)?;
                *prev_order = scene.pages.keys().copied().collect();
                reorder_indexmap(&mut scene.pages, order);
            }

            Op::AddNode { page, node, at } => {
                let page_ref = scene.page_mut(*page).ok_or(OpError::PageNotFound(*page))?;
                if page_ref.nodes.contains_key(&node.id) {
                    return Err(OpError::NodeExists(node.id));
                }
                let len = page_ref.nodes.len();
                if *at > len {
                    return Err(OpError::IndexOutOfRange { index: *at, len });
                }
                page_ref.nodes.insert(node.id, node.clone());
                let last = page_ref.nodes.len() - 1;
                if *at < last {
                    page_ref.nodes.move_index(last, *at);
                }
                validate_page_invariants(page_ref)?;
            }

            Op::RemoveNode {
                page,
                id,
                prev_node,
                prev_index,
            } => {
                let page_ref = scene.page_mut(*page).ok_or(OpError::PageNotFound(*page))?;
                let index = page_ref
                    .nodes
                    .get_index_of(id)
                    .ok_or(OpError::NodeNotFound {
                        page: *page,
                        node: *id,
                    })?;
                let (_, node) =
                    page_ref
                        .nodes
                        .shift_remove_index(index)
                        .ok_or(OpError::NodeNotFound {
                            page: *page,
                            node: *id,
                        })?;
                *prev_node = node;
                *prev_index = index;
            }

            Op::UpdateNode {
                page,
                id,
                patch,
                prev,
            } => {
                let node = scene.node_mut(*page, *id).ok_or(OpError::NodeNotFound {
                    page: *page,
                    node: *id,
                })?;
                if let Some(data_patch) = &patch.data {
                    let existing = node.kind.discriminant();
                    if existing != data_patch.tag() {
                        return Err(OpError::NodeKindMismatch {
                            patch: data_patch.tag(),
                            existing,
                        });
                    }
                }
                *prev = capture_prev_node_patch(node, patch);
                apply_node_patch(node, patch);
            }

            Op::ReorderNodes {
                page,
                order,
                prev_order,
            } => {
                let page_ref = scene.page_mut(*page).ok_or(OpError::PageNotFound(*page))?;
                ensure_same_node_set(page_ref, order)?;
                *prev_order = page_ref.nodes.keys().copied().collect();
                reorder_indexmap(&mut page_ref.nodes, order);
            }

            Op::Batch { ops, .. } => {
                for op in ops.iter_mut() {
                    op.apply(scene)?;
                }
            }
        }
        Ok(())
    }

    /// Compute the inverse Op from this Op's `prev_*` fields. Assumes `apply`
    /// has already run on this Op (so `prev_*` are populated).
    pub fn inverse(&self) -> Op {
        match self {
            Op::UpdateProjectMeta { prev, patch } => Op::UpdateProjectMeta {
                patch: prev.clone(),
                prev: patch.clone(),
            },
            Op::AddPage { page, at } => Op::RemovePage {
                id: page.id,
                prev_page: page.clone(),
                prev_index: *at,
            },
            Op::RemovePage {
                prev_page,
                prev_index,
                ..
            } => Op::AddPage {
                page: prev_page.clone(),
                at: *prev_index,
            },
            Op::UpdatePage { id, patch, prev } => Op::UpdatePage {
                id: *id,
                patch: prev.clone(),
                prev: patch.clone(),
            },
            Op::ReorderPages { order, prev_order } => Op::ReorderPages {
                order: prev_order.clone(),
                prev_order: order.clone(),
            },
            Op::AddNode { page, node, at } => Op::RemoveNode {
                page: *page,
                id: node.id,
                prev_node: node.clone(),
                prev_index: *at,
            },
            Op::RemoveNode {
                page,
                prev_node,
                prev_index,
                ..
            } => Op::AddNode {
                page: *page,
                node: prev_node.clone(),
                at: *prev_index,
            },
            Op::UpdateNode {
                page,
                id,
                patch,
                prev,
            } => Op::UpdateNode {
                page: *page,
                id: *id,
                patch: prev.clone(),
                prev: patch.clone(),
            },
            Op::ReorderNodes {
                page,
                order,
                prev_order,
            } => Op::ReorderNodes {
                page: *page,
                order: prev_order.clone(),
                prev_order: order.clone(),
            },
            Op::Batch { ops, label } => Op::Batch {
                // Inverse batch runs ops in reverse order.
                ops: ops.iter().rev().map(Op::inverse).collect(),
                label: format!("undo: {label}"),
            },
        }
    }

    /// Pre-flight validation. Currently lightweight — `apply` re-validates
    /// inline. Hook for stricter rule checks without mutation.
    pub fn validate(&self, scene: &Scene) -> OpResult {
        match self {
            Op::UpdateProjectMeta { .. } => Ok(()),
            Op::AddPage { page, at } => {
                if scene.pages.contains_key(&page.id) {
                    return Err(OpError::PageExists(page.id));
                }
                if *at > scene.pages.len() {
                    return Err(OpError::IndexOutOfRange {
                        index: *at,
                        len: scene.pages.len(),
                    });
                }
                Ok(())
            }
            Op::RemovePage { id, .. } | Op::UpdatePage { id, .. } => scene
                .page(*id)
                .ok_or(OpError::PageNotFound(*id))
                .map(|_| ()),
            Op::ReorderPages { order, .. } => ensure_same_page_set(&scene.pages, order),
            Op::AddNode { page, node, at } => {
                let page_ref = scene.page(*page).ok_or(OpError::PageNotFound(*page))?;
                if page_ref.nodes.contains_key(&node.id) {
                    return Err(OpError::NodeExists(node.id));
                }
                if *at > page_ref.nodes.len() {
                    return Err(OpError::IndexOutOfRange {
                        index: *at,
                        len: page_ref.nodes.len(),
                    });
                }
                Ok(())
            }
            Op::RemoveNode { page, id, .. } => {
                let page_ref = scene.page(*page).ok_or(OpError::PageNotFound(*page))?;
                if !page_ref.nodes.contains_key(id) {
                    return Err(OpError::NodeNotFound {
                        page: *page,
                        node: *id,
                    });
                }
                Ok(())
            }
            Op::UpdateNode {
                page, id, patch, ..
            } => {
                let node = scene.node(*page, *id).ok_or(OpError::NodeNotFound {
                    page: *page,
                    node: *id,
                })?;
                if let Some(data_patch) = &patch.data {
                    let existing = node.kind.discriminant();
                    if existing != data_patch.tag() {
                        return Err(OpError::NodeKindMismatch {
                            patch: data_patch.tag(),
                            existing,
                        });
                    }
                }
                Ok(())
            }
            Op::ReorderNodes { page, order, .. } => {
                let page_ref = scene.page(*page).ok_or(OpError::PageNotFound(*page))?;
                ensure_same_node_set(page_ref, order)
            }
            Op::Batch { ops, .. } => {
                for op in ops {
                    op.validate(scene)?;
                }
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ensure_same_page_set(pages: &indexmap::IndexMap<PageId, Page>, order: &[PageId]) -> OpResult {
    if order.len() != pages.len() {
        return Err(OpError::ReorderSetMismatch);
    }
    for id in order {
        if !pages.contains_key(id) {
            return Err(OpError::ReorderSetMismatch);
        }
    }
    Ok(())
}

fn ensure_same_node_set(page: &Page, order: &[NodeId]) -> OpResult {
    if order.len() != page.nodes.len() {
        return Err(OpError::ReorderSetMismatch);
    }
    for id in order {
        if !page.nodes.contains_key(id) {
            return Err(OpError::ReorderSetMismatch);
        }
    }
    Ok(())
}

fn reorder_indexmap<K: Copy + std::hash::Hash + Eq, V>(
    map: &mut indexmap::IndexMap<K, V>,
    order: &[K],
) {
    // `order` is guaranteed to be a permutation of existing keys (caller checks).
    for (target_index, key) in order.iter().enumerate() {
        if let Some(current_index) = map.get_index_of(key)
            && current_index != target_index
        {
            map.move_index(current_index, target_index);
        }
    }
}

fn validate_page_invariants(page: &Page) -> OpResult {
    let mut source = 0usize;
    let mut inpainted = 0usize;
    let mut rendered = 0usize;
    let mut seg = 0usize;
    let mut brush = 0usize;
    let mut bubble = 0usize;
    for node in page.nodes.values() {
        match &node.kind {
            NodeKind::Image(img) => match img.role {
                ImageRole::Source => source += 1,
                ImageRole::Inpainted => inpainted += 1,
                ImageRole::Rendered => rendered += 1,
                ImageRole::Custom => {}
            },
            NodeKind::Mask(mask) => match mask.role {
                MaskRole::Segment => seg += 1,
                MaskRole::BrushInpaint => brush += 1,
                MaskRole::Bubble => bubble += 1,
            },
            NodeKind::Text(_) => {}
        }
    }
    // Source = 0 is allowed transiently during construction; validated by the caller.
    if source > 1 {
        return Err(OpError::Invariant("more than one Source image on page"));
    }
    if inpainted > 1 {
        return Err(OpError::Invariant("more than one Inpainted image on page"));
    }
    if rendered > 1 {
        return Err(OpError::Invariant("more than one Rendered image on page"));
    }
    if seg > 1 {
        return Err(OpError::Invariant("more than one Segment mask on page"));
    }
    if brush > 1 {
        return Err(OpError::Invariant(
            "more than one BrushInpaint mask on page",
        ));
    }
    if bubble > 1 {
        return Err(OpError::Invariant("more than one Bubble mask on page"));
    }
    Ok(())
}

fn capture_prev_node_patch(node: &Node, patch: &NodePatch) -> NodePatch {
    NodePatch {
        transform: patch.transform.as_ref().map(|_| node.transform),
        visible: patch.visible.map(|_| node.visible),
        data: patch.data.as_ref().map(|data_patch| match data_patch {
            NodeDataPatch::Text(p) => NodeDataPatch::Text(capture_prev_text(&node.kind, p)),
            NodeDataPatch::Image(p) => NodeDataPatch::Image(capture_prev_image(&node.kind, p)),
            NodeDataPatch::Mask(p) => NodeDataPatch::Mask(capture_prev_mask(&node.kind, p)),
        }),
    }
}

fn capture_prev_text(kind: &NodeKind, p: &TextDataPatch) -> TextDataPatch {
    let NodeKind::Text(data) = kind else {
        return TextDataPatch::default();
    };
    TextDataPatch {
        confidence: p.confidence.as_ref().map(|_| data.confidence),
        source_lang: p.source_lang.as_ref().map(|_| data.source_lang.clone()),
        source_direction: p.source_direction.as_ref().map(|_| data.source_direction),
        rendered_direction: p
            .rendered_direction
            .as_ref()
            .map(|_| data.rendered_direction),
        line_polygons: p.line_polygons.as_ref().map(|_| data.line_polygons.clone()),
        rotation_deg: p.rotation_deg.as_ref().map(|_| data.rotation_deg),
        detected_font_size_px: p
            .detected_font_size_px
            .as_ref()
            .map(|_| data.detected_font_size_px),
        detector: p.detector.as_ref().map(|_| data.detector.clone()),
        text: p.text.as_ref().map(|_| data.text.clone()),
        translation: p.translation.as_ref().map(|_| data.translation.clone()),
        style: p.style.as_ref().map(|_| data.style.clone()),
        font_prediction: p
            .font_prediction
            .as_ref()
            .map(|_| data.font_prediction.clone()),
        sprite: p.sprite.as_ref().map(|_| data.sprite.clone()),
        sprite_transform: p.sprite_transform.as_ref().map(|_| data.sprite_transform),
        lock_layout_box: p.lock_layout_box.as_ref().map(|_| data.lock_layout_box),
    }
}

fn capture_prev_image(kind: &NodeKind, p: &ImageDataPatch) -> ImageDataPatch {
    let NodeKind::Image(data) = kind else {
        return ImageDataPatch::default();
    };
    ImageDataPatch {
        blob: p.blob.as_ref().map(|_| data.blob.clone()),
        opacity: p.opacity.as_ref().map(|_| data.opacity),
        name: p.name.as_ref().map(|_| data.name.clone()),
        natural_width: p.natural_width.as_ref().map(|_| data.natural_width),
        natural_height: p.natural_height.as_ref().map(|_| data.natural_height),
    }
}

fn capture_prev_mask(kind: &NodeKind, p: &MaskDataPatch) -> MaskDataPatch {
    let NodeKind::Mask(data) = kind else {
        return MaskDataPatch::default();
    };
    MaskDataPatch {
        blob: p.blob.as_ref().map(|_| data.blob.clone()),
    }
}

fn apply_node_patch(node: &mut Node, patch: &NodePatch) {
    if let Some(t) = patch.transform {
        node.transform = t;
    }
    if let Some(v) = patch.visible {
        node.visible = v;
    }
    if let Some(data_patch) = &patch.data {
        match (&mut node.kind, data_patch) {
            (NodeKind::Text(t), NodeDataPatch::Text(p)) => apply_text_patch(t, p),
            (NodeKind::Image(i), NodeDataPatch::Image(p)) => apply_image_patch(i, p),
            (NodeKind::Mask(m), NodeDataPatch::Mask(p)) => apply_mask_patch(m, p),
            _ => {
                // Kind mismatch was validated in apply(); unreachable.
            }
        }
    }
}

fn apply_text_patch(t: &mut TextData, p: &TextDataPatch) {
    if let Some(v) = p.confidence {
        t.confidence = v;
    }
    if let Some(v) = &p.source_lang {
        t.source_lang = v.clone();
    }
    if let Some(v) = p.source_direction {
        t.source_direction = v;
    }
    if let Some(v) = p.rendered_direction {
        t.rendered_direction = v;
    }
    if let Some(v) = &p.line_polygons {
        t.line_polygons = v.clone();
    }
    if let Some(v) = p.rotation_deg {
        t.rotation_deg = v;
    }
    if let Some(v) = p.detected_font_size_px {
        t.detected_font_size_px = v;
    }
    if let Some(v) = &p.detector {
        t.detector = v.clone();
    }
    if let Some(v) = &p.text {
        t.text = v.clone();
    }
    if let Some(v) = &p.translation {
        t.translation = v.clone();
    }
    if let Some(v) = &p.style {
        t.style = v.clone();
    }
    if let Some(v) = &p.font_prediction {
        t.font_prediction = v.clone();
    }
    if let Some(v) = &p.sprite {
        t.sprite = v.clone();
    }
    if let Some(v) = p.sprite_transform {
        t.sprite_transform = v;
    }
    if let Some(v) = p.lock_layout_box {
        t.lock_layout_box = v;
    }
}

fn apply_image_patch(i: &mut ImageData, p: &ImageDataPatch) {
    if let Some(v) = &p.blob {
        i.blob = v.clone();
    }
    if let Some(v) = p.opacity {
        i.opacity = v;
    }
    if let Some(v) = &p.name {
        i.name = v.clone();
    }
    if let Some(v) = p.natural_width {
        i.natural_width = v;
    }
    if let Some(v) = p.natural_height {
        i.natural_height = v;
    }
}

fn apply_mask_patch(m: &mut MaskData, p: &MaskDataPatch) {
    if let Some(v) = &p.blob {
        m.blob = v.clone();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{ImageData, ImageRole, Node, NodeKind, Page};

    fn seed_scene() -> Scene {
        Scene::default()
    }

    fn blank_page() -> Page {
        Page::new("p1", 800, 1200)
    }

    fn custom_image_node() -> Node {
        Node {
            id: NodeId::new(),
            transform: Transform::default(),
            visible: true,
            kind: NodeKind::Image(ImageData {
                role: ImageRole::Custom,
                blob: BlobRef::new("deadbeef"),
                opacity: 1.0,
                natural_width: 100,
                natural_height: 100,
                name: Some("layer.png".into()),
            }),
        }
    }

    #[test]
    fn add_then_undo_add_restores_scene() {
        let mut scene = seed_scene();
        let page = blank_page();
        let mut op = Op::AddPage {
            page: page.clone(),
            at: 0,
        };
        op.apply(&mut scene).unwrap();
        assert_eq!(scene.pages.len(), 1);
        let mut undo = op.inverse();
        undo.apply(&mut scene).unwrap();
        assert!(scene.pages.is_empty());
    }

    #[test]
    fn update_node_then_undo_round_trips_transform() {
        let mut scene = seed_scene();
        let page = blank_page();
        let page_id = page.id;
        Op::AddPage { page, at: 0 }.apply(&mut scene).unwrap();

        let node = custom_image_node();
        let node_id = node.id;
        Op::AddNode {
            page: page_id,
            node,
            at: 0,
        }
        .apply(&mut scene)
        .unwrap();

        let new_transform = Transform {
            x: 50.0,
            y: 60.0,
            width: 10.0,
            height: 10.0,
            rotation_deg: 0.0,
        };
        let mut op = Op::UpdateNode {
            page: page_id,
            id: node_id,
            patch: NodePatch {
                transform: Some(new_transform),
                ..Default::default()
            },
            prev: NodePatch::default(),
        };
        op.apply(&mut scene).unwrap();
        assert_eq!(
            scene.node(page_id, node_id).unwrap().transform.x,
            new_transform.x
        );

        let mut undo = op.inverse();
        undo.apply(&mut scene).unwrap();
        assert_eq!(scene.node(page_id, node_id).unwrap().transform.x, 0.0);
    }

    #[test]
    fn reject_two_source_images_on_one_page() {
        let mut scene = seed_scene();
        let page = blank_page();
        let page_id = page.id;
        Op::AddPage { page, at: 0 }.apply(&mut scene).unwrap();

        let src1 = Node {
            id: NodeId::new(),
            transform: Transform::default(),
            visible: true,
            kind: NodeKind::Image(ImageData {
                role: ImageRole::Source,
                blob: BlobRef::new("a"),
                opacity: 1.0,
                natural_width: 10,
                natural_height: 10,
                name: None,
            }),
        };
        let src2 = Node {
            id: NodeId::new(),
            transform: Transform::default(),
            visible: true,
            kind: NodeKind::Image(ImageData {
                role: ImageRole::Source,
                blob: BlobRef::new("b"),
                opacity: 1.0,
                natural_width: 10,
                natural_height: 10,
                name: None,
            }),
        };
        Op::AddNode {
            page: page_id,
            node: src1,
            at: 0,
        }
        .apply(&mut scene)
        .unwrap();
        let result = Op::AddNode {
            page: page_id,
            node: src2,
            at: 1,
        }
        .apply(&mut scene);
        assert!(matches!(result, Err(OpError::Invariant(_))));
    }

    #[test]
    fn reorder_pages_round_trips() {
        let mut scene = seed_scene();
        let ids: Vec<PageId> = (0..3)
            .map(|i| {
                let page = Page::new(format!("p{i}"), 10, 10);
                let id = page.id;
                Op::AddPage { page, at: i }.apply(&mut scene).unwrap();
                id
            })
            .collect();

        let reversed: Vec<_> = ids.iter().rev().copied().collect();
        let mut op = Op::ReorderPages {
            order: reversed.clone(),
            prev_order: Vec::new(),
        };
        op.apply(&mut scene).unwrap();
        assert_eq!(scene.pages.keys().copied().collect::<Vec<_>>(), reversed);

        let mut undo = op.inverse();
        undo.apply(&mut scene).unwrap();
        assert_eq!(scene.pages.keys().copied().collect::<Vec<_>>(), ids);
    }
}
