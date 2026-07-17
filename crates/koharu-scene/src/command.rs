use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::{
    BlobId, CanvasSize, CommandId, Error, ImageNode, MaskNode, Node, NodeBuilder, NodeId, NodeKind,
    Page, PageId, PixelSize, Result, Revision, Scene, TextLayout, TextNode, TextStyle, Transform,
    node::BuilderKind,
    scene::{PageSnapshot, Placement, PlacementParent, SubtreeSnapshot},
};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Parent {
    Page(PageId),
    Node(NodeId),
}

impl From<Parent> for PlacementParent {
    fn from(value: Parent) -> Self {
        match value {
            Parent::Page(page) => Self::Page(page),
            Parent::Node(node) => Self::Node(node),
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Position {
    #[default]
    Top,
    Bottom,
    Above(NodeId),
    Below(NodeId),
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PagePosition {
    First,
    #[default]
    Last,
    Before(PageId),
    After(PageId),
}

#[derive(Clone)]
pub struct CommandBatch {
    pub(crate) id: CommandId,
    pub(crate) base_revision: Revision,
    pub(crate) ops: Vec<PendingOp>,
    pub(crate) attachments: HashMap<BlobId, Arc<[u8]>>,
    created_pages: HashSet<PageId>,
    created_nodes: HashSet<NodeId>,
}

impl CommandBatch {
    #[must_use]
    pub fn new(base_revision: Revision) -> Self {
        Self::with_id(base_revision, CommandId::new())
    }

    #[must_use]
    pub fn with_id(base_revision: Revision, id: CommandId) -> Self {
        Self {
            id,
            base_revision,
            ops: Vec::new(),
            attachments: HashMap::new(),
            created_pages: HashSet::new(),
            created_nodes: HashSet::new(),
        }
    }

    #[must_use]
    pub const fn id(&self) -> CommandId {
        self.id
    }

    #[must_use]
    pub const fn base_revision(&self) -> Revision {
        self.base_revision
    }

    pub fn create_page(&mut self, page: Page) -> Result<PageId> {
        let page = page.into_empty_snapshot()?;
        let id = page.id;
        self.created_pages.insert(id);
        self.ops.push(PendingOp::CreatePage { page });
        Ok(id)
    }

    pub fn remove_page(&mut self, page: PageId) -> Result<()> {
        self.ops.push(PendingOp::RemovePage { page });
        Ok(())
    }

    pub fn move_page(&mut self, page: PageId, position: PagePosition) -> Result<()> {
        self.ops.push(PendingOp::MovePage { page, position });
        Ok(())
    }

    pub fn rename_page(&mut self, page: PageId, name: impl Into<String>) -> Result<()> {
        self.ops.push(PendingOp::RenamePage {
            page,
            name: name.into(),
        });
        Ok(())
    }

    pub fn resize_page(&mut self, page: PageId, size: CanvasSize) -> Result<()> {
        self.ops.push(PendingOp::ResizePage { page, size });
        Ok(())
    }

    pub fn create(
        &mut self,
        parent: Parent,
        position: Position,
        builder: NodeBuilder,
    ) -> Result<NodeId> {
        let node = self.consume_builder(builder)?;
        let id = node.id;
        self.created_nodes.insert(id);
        self.ops.push(PendingOp::CreateNode {
            parent,
            position,
            node,
        });
        Ok(id)
    }

    pub fn remove_node(&mut self, node: NodeId) -> Result<()> {
        self.ops.push(PendingOp::RemoveNode { node });
        Ok(())
    }

    pub fn move_node(&mut self, node: NodeId, parent: Parent, position: Position) -> Result<()> {
        self.ops.push(PendingOp::MoveNode {
            node,
            parent,
            position,
        });
        Ok(())
    }

    pub fn place_above(&mut self, node: NodeId, anchor: NodeId) -> Result<()> {
        self.ops.push(PendingOp::PlaceRelative {
            node,
            anchor,
            above: true,
        });
        Ok(())
    }

    pub fn place_below(&mut self, node: NodeId, anchor: NodeId) -> Result<()> {
        self.ops.push(PendingOp::PlaceRelative {
            node,
            anchor,
            above: false,
        });
        Ok(())
    }

    pub fn set_name(&mut self, node: NodeId, name: Option<String>) -> Result<()> {
        self.ops.push(PendingOp::SetName { node, name });
        Ok(())
    }

    pub fn set_visible(&mut self, node: NodeId, visible: bool) -> Result<()> {
        self.ops.push(PendingOp::SetVisible { node, visible });
        Ok(())
    }

    pub fn set_opacity(&mut self, node: NodeId, opacity: f32) -> Result<()> {
        self.ops.push(PendingOp::SetOpacity { node, opacity });
        Ok(())
    }

    pub fn set_transform(&mut self, node: NodeId, transform: Transform) -> Result<()> {
        self.ops.push(PendingOp::SetTransform { node, transform });
        Ok(())
    }

    pub fn set_image(&mut self, node: NodeId, bytes: impl Into<Arc<[u8]>>) -> Result<()> {
        let blob = self.attach(bytes.into())?;
        self.ops.push(PendingOp::SetImage { node, blob });
        Ok(())
    }

    pub fn set_mask(&mut self, node: NodeId, bytes: impl Into<Arc<[u8]>>) -> Result<()> {
        let blob = self.attach(bytes.into())?;
        self.ops.push(PendingOp::SetMask { node, blob });
        Ok(())
    }

    pub fn set_text(&mut self, node: NodeId, text: impl Into<String>) -> Result<()> {
        self.ops.push(PendingOp::SetText {
            node,
            text: text.into(),
        });
        Ok(())
    }

    pub fn set_text_style(&mut self, node: NodeId, style: TextStyle) -> Result<()> {
        self.ops.push(PendingOp::SetTextStyle { node, style });
        Ok(())
    }

    pub fn set_text_layout(&mut self, node: NodeId, layout: TextLayout) -> Result<()> {
        self.ops.push(PendingOp::SetTextLayout { node, layout });
        Ok(())
    }

    fn consume_builder(&mut self, builder: NodeBuilder) -> Result<PendingNode> {
        let kind = match builder.kind {
            BuilderKind::Group => PendingNodeKind::Group,
            BuilderKind::Image(bytes) => PendingNodeKind::Image(self.attach(bytes)?),
            BuilderKind::Mask(bytes) => PendingNodeKind::Mask(self.attach(bytes)?),
            BuilderKind::Text(text) => PendingNodeKind::Text(text),
        };
        Ok(PendingNode {
            id: builder.id,
            name: builder.name,
            visible: builder.visible,
            opacity: builder.opacity,
            transform: builder.transform,
            kind,
        })
    }

    fn attach(&mut self, bytes: Arc<[u8]>) -> Result<BlobId> {
        let id = BlobId::from_bytes(&bytes);
        if let Some(existing) = self.attachments.get(&id) {
            if existing.as_ref() != bytes.as_ref() {
                return Err(Error::invalid("BLAKE3 collision in command attachments"));
            }
        } else {
            self.attachments.insert(id, bytes);
        }
        Ok(id)
    }

    pub(crate) fn request_hash(&self) -> Result<[u8; 32]> {
        let request = PendingRequest {
            version: 1,
            base_revision: self.base_revision,
            ops: &self.ops,
        };
        let bytes = postcard::to_stdvec(&request)?;
        Ok(*blake3::hash(&bytes).as_bytes())
    }

    pub(crate) fn creates_page(&self, id: PageId) -> bool {
        self.created_pages.contains(&id)
    }

    pub(crate) fn creates_node(&self, id: NodeId) -> bool {
        self.created_nodes.contains(&id)
    }
}

#[derive(Serialize)]
struct PendingRequest<'a> {
    version: u8,
    base_revision: Revision,
    ops: &'a [PendingOp],
}

#[derive(Clone, Debug, Serialize)]
pub(crate) enum PendingOp {
    CreatePage {
        page: PageSnapshot,
    },
    RemovePage {
        page: PageId,
    },
    MovePage {
        page: PageId,
        position: PagePosition,
    },
    RenamePage {
        page: PageId,
        name: String,
    },
    ResizePage {
        page: PageId,
        size: CanvasSize,
    },
    CreateNode {
        parent: Parent,
        position: Position,
        node: PendingNode,
    },
    RemoveNode {
        node: NodeId,
    },
    MoveNode {
        node: NodeId,
        parent: Parent,
        position: Position,
    },
    PlaceRelative {
        node: NodeId,
        anchor: NodeId,
        above: bool,
    },
    SetName {
        node: NodeId,
        name: Option<String>,
    },
    SetVisible {
        node: NodeId,
        visible: bool,
    },
    SetOpacity {
        node: NodeId,
        opacity: f32,
    },
    SetTransform {
        node: NodeId,
        transform: Transform,
    },
    SetImage {
        node: NodeId,
        blob: BlobId,
    },
    SetMask {
        node: NodeId,
        blob: BlobId,
    },
    SetText {
        node: NodeId,
        text: String,
    },
    SetTextStyle {
        node: NodeId,
        style: TextStyle,
    },
    SetTextLayout {
        node: NodeId,
        layout: TextLayout,
    },
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PendingNode {
    pub id: NodeId,
    pub name: Option<String>,
    pub visible: bool,
    pub opacity: f32,
    pub transform: Transform,
    pub kind: PendingNodeKind,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) enum PendingNodeKind {
    Group,
    Mask(BlobId),
    Image(BlobId),
    Text(Box<TextNode>),
}

impl PendingNode {
    pub(crate) fn into_node(
        self,
        mut image_size: impl FnMut(BlobId, bool) -> Result<PixelSize>,
    ) -> Result<Node> {
        let kind = match self.kind {
            PendingNodeKind::Group => NodeKind::Group,
            PendingNodeKind::Mask(blob) => NodeKind::Mask(MaskNode {
                blob,
                natural_size: image_size(blob, true)?,
            }),
            PendingNodeKind::Image(blob) => NodeKind::Image(ImageNode {
                blob,
                natural_size: image_size(blob, false)?,
            }),
            PendingNodeKind::Text(text) => NodeKind::Text(text),
        };
        Ok(Node {
            id: self.id,
            name: self.name,
            visible: self.visible,
            opacity: self.opacity,
            transform: self.transform,
            kind,
        })
    }
}

pub(crate) fn resolve_placement(
    scene: &Scene,
    parent: Parent,
    position: Position,
) -> Result<Placement> {
    let parent: PlacementParent = parent.into();
    let before = match position {
        Position::Top => None,
        Position::Bottom => scene.first_child(&parent)?,
        Position::Below(anchor) => {
            let anchor_placement = scene.placement_of(anchor)?;
            if anchor_placement.parent != parent {
                return Err(Error::invalid("relative anchor has a different parent"));
            }
            Some(anchor)
        }
        Position::Above(anchor) => {
            let anchor_placement = scene.placement_of(anchor)?;
            if anchor_placement.parent != parent {
                return Err(Error::invalid("relative anchor has a different parent"));
            }
            anchor_placement.before
        }
    };
    Ok(Placement { parent, before })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StoredBatch {
    pub version: u8,
    pub ops: Vec<StoredOp>,
}

impl StoredBatch {
    pub(crate) fn new(ops: Vec<StoredOp>) -> Self {
        Self { version: 1, ops }
    }

    pub(crate) fn into_inverse(self) -> Self {
        Self::new(
            self.ops
                .into_iter()
                .rev()
                .map(StoredOp::into_inverse)
                .collect(),
        )
    }

    pub(crate) fn blob_ids(&self) -> Vec<BlobId> {
        let mut ids = self
            .ops
            .iter()
            .flat_map(StoredOp::blob_ids)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum StoredOp {
    RestorePage {
        page: PageSnapshot,
        index: usize,
    },
    RemovePage {
        page: PageSnapshot,
        index: usize,
    },
    MovePage {
        page: PageId,
        from: usize,
        to: usize,
    },
    RenamePage {
        page: PageId,
        from: String,
        to: String,
    },
    ResizePage {
        page: PageId,
        from: CanvasSize,
        to: CanvasSize,
    },
    RestoreSubtree {
        subtree: SubtreeSnapshot,
        placement: Placement,
    },
    RemoveSubtree {
        subtree: SubtreeSnapshot,
        placement: Placement,
    },
    MoveSubtree {
        node: NodeId,
        from: Placement,
        to: Placement,
    },
    SetName {
        node: NodeId,
        from: Option<String>,
        to: Option<String>,
    },
    SetVisible {
        node: NodeId,
        from: bool,
        to: bool,
    },
    SetOpacity {
        node: NodeId,
        from: f32,
        to: f32,
    },
    SetTransform {
        node: NodeId,
        from: Transform,
        to: Transform,
    },
    SetImage {
        node: NodeId,
        from: ImageNode,
        to: ImageNode,
    },
    SetMask {
        node: NodeId,
        from: MaskNode,
        to: MaskNode,
    },
    SetText {
        node: NodeId,
        from: String,
        to: String,
    },
    SetTextStyle {
        node: NodeId,
        from: TextStyle,
        to: TextStyle,
    },
    SetTextLayout {
        node: NodeId,
        from: TextLayout,
        to: TextLayout,
    },
}

impl StoredOp {
    fn into_inverse(self) -> Self {
        match self {
            Self::RestorePage { page, index } => Self::RemovePage { page, index },
            Self::RemovePage { page, index } => Self::RestorePage { page, index },
            Self::MovePage { page, from, to } => Self::MovePage {
                page,
                from: to,
                to: from,
            },
            Self::RenamePage { page, from, to } => Self::RenamePage {
                page,
                from: to,
                to: from,
            },
            Self::ResizePage { page, from, to } => Self::ResizePage {
                page,
                from: to,
                to: from,
            },
            Self::RestoreSubtree { subtree, placement } => {
                Self::RemoveSubtree { subtree, placement }
            }
            Self::RemoveSubtree { subtree, placement } => {
                Self::RestoreSubtree { subtree, placement }
            }
            Self::MoveSubtree { node, from, to } => Self::MoveSubtree {
                node,
                from: to,
                to: from,
            },
            Self::SetName { node, from, to } => Self::SetName {
                node,
                from: to,
                to: from,
            },
            Self::SetVisible { node, from, to } => Self::SetVisible {
                node,
                from: to,
                to: from,
            },
            Self::SetOpacity { node, from, to } => Self::SetOpacity {
                node,
                from: to,
                to: from,
            },
            Self::SetTransform { node, from, to } => Self::SetTransform {
                node,
                from: to,
                to: from,
            },
            Self::SetImage { node, from, to } => Self::SetImage {
                node,
                from: to,
                to: from,
            },
            Self::SetMask { node, from, to } => Self::SetMask {
                node,
                from: to,
                to: from,
            },
            Self::SetText { node, from, to } => Self::SetText {
                node,
                from: to,
                to: from,
            },
            Self::SetTextStyle { node, from, to } => Self::SetTextStyle {
                node,
                from: to,
                to: from,
            },
            Self::SetTextLayout { node, from, to } => Self::SetTextLayout {
                node,
                from: to,
                to: from,
            },
        }
    }

    pub(crate) fn apply(&self, scene: &mut Scene) -> Result<()> {
        self.apply_direction(scene, false)
    }

    pub(crate) fn apply_backward(&self, scene: &mut Scene) -> Result<()> {
        self.apply_direction(scene, true)
    }

    fn apply_direction(&self, scene: &mut Scene, backward: bool) -> Result<()> {
        match self {
            Self::RestorePage { page, index } | Self::RemovePage { page, index } => {
                let restore = matches!(self, Self::RestorePage { .. }) ^ backward;
                if restore {
                    scene.restore_page(page, *index)
                } else {
                    let (actual_index, actual) = scene.remove_page(page.id)?;
                    if actual_index != *index || actual != *page {
                        scene
                            .restore_page(&actual, actual_index)
                            .expect("removed page must be restorable");
                        return Err(Error::HistoryConflict(format!(
                            "page {} changed before removal",
                            page.id
                        )));
                    }
                    Ok(())
                }
            }
            Self::MovePage { page, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if scene.page_index(*page)? != *from {
                    return Err(Error::HistoryConflict(format!(
                        "page {page} moved before history operation"
                    )));
                }
                scene.move_page(*page, *to)?;
                Ok(())
            }
            Self::RenamePage { page, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if scene.page_data(*page)?.name() != from {
                    return Err(Error::HistoryConflict(format!("page {page} name changed")));
                }
                scene.rename_page(*page, to.clone())?;
                Ok(())
            }
            Self::ResizePage { page, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if scene.page_data(*page)?.size() != *from {
                    return Err(Error::HistoryConflict(format!("page {page} size changed")));
                }
                scene.resize_page(*page, *to)?;
                Ok(())
            }
            Self::RestoreSubtree { subtree, placement }
            | Self::RemoveSubtree { subtree, placement } => {
                let restore = matches!(self, Self::RestoreSubtree { .. }) ^ backward;
                if restore {
                    scene.restore_subtree(subtree, placement)
                } else {
                    let root = subtree
                        .nodes
                        .first()
                        .ok_or_else(|| Error::invalid("empty subtree in history"))?
                        .node
                        .id;
                    let (actual_placement, actual) = scene.remove_subtree(root)?;
                    if actual_placement != *placement || actual != *subtree {
                        scene
                            .restore_subtree(&actual, &actual_placement)
                            .expect("removed subtree must be restorable");
                        return Err(Error::HistoryConflict(format!(
                            "subtree {root} changed before removal"
                        )));
                    }
                    Ok(())
                }
            }
            Self::MoveSubtree { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if scene.placement_of(*node)? != *from {
                    return Err(Error::HistoryConflict(format!(
                        "node {node} moved before history operation"
                    )));
                }
                scene.move_subtree(*node, to)?;
                Ok(())
            }
            Self::SetName { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if scene.node(*node)?.name() != from.as_deref() {
                    return Err(Error::HistoryConflict(format!("node {node} name changed")));
                }
                scene.set_name(*node, to.clone())?;
                Ok(())
            }
            Self::SetVisible { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if scene.node(*node)?.visible() != *from {
                    return Err(Error::HistoryConflict(format!(
                        "node {node} visibility changed"
                    )));
                }
                scene.set_visible(*node, *to)?;
                Ok(())
            }
            Self::SetOpacity { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if scene.node(*node)?.opacity() != *from {
                    return Err(Error::HistoryConflict(format!(
                        "node {node} opacity changed"
                    )));
                }
                scene.set_opacity(*node, *to)?;
                Ok(())
            }
            Self::SetTransform { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if scene.node(*node)?.transform() != *from {
                    return Err(Error::HistoryConflict(format!(
                        "node {node} transform changed"
                    )));
                }
                scene.set_transform(*node, *to)?;
                Ok(())
            }
            Self::SetImage { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if !matches!(scene.node(*node)?.kind(), NodeKind::Image(value) if value == from) {
                    return Err(Error::HistoryConflict(format!(
                        "node {node} content changed"
                    )));
                }
                scene.set_image(*node, to.clone())?;
                Ok(())
            }
            Self::SetMask { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if !matches!(scene.node(*node)?.kind(), NodeKind::Mask(value) if value == from) {
                    return Err(Error::HistoryConflict(format!(
                        "node {node} content changed"
                    )));
                }
                scene.set_mask(*node, to.clone())?;
                Ok(())
            }
            Self::SetText { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if !matches!(scene.node(*node)?.kind(), NodeKind::Text(value) if value.text == *from)
                {
                    return Err(Error::HistoryConflict(format!("node {node} text changed")));
                }
                scene.set_text(*node, to.clone())?;
                Ok(())
            }
            Self::SetTextStyle { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if !matches!(scene.node(*node)?.kind(), NodeKind::Text(value) if value.style == *from)
                {
                    return Err(Error::HistoryConflict(format!(
                        "node {node} text style changed"
                    )));
                }
                scene.set_text_style(*node, to.clone())?;
                Ok(())
            }
            Self::SetTextLayout { node, from, to } => {
                let (from, to) = if backward { (to, from) } else { (from, to) };
                if !matches!(scene.node(*node)?.kind(), NodeKind::Text(value) if value.layout == *from)
                {
                    return Err(Error::HistoryConflict(format!(
                        "node {node} text layout changed"
                    )));
                }
                scene.set_text_layout(*node, to.clone())?;
                Ok(())
            }
        }
    }

    fn blob_ids(&self) -> Vec<BlobId> {
        fn node_blobs(node: &Node, ids: &mut Vec<BlobId>) {
            match &node.kind {
                NodeKind::Image(image) => ids.push(image.blob),
                NodeKind::Mask(mask) => ids.push(mask.blob),
                NodeKind::Group | NodeKind::Text(_) => {}
            }
        }

        let mut ids = Vec::new();
        match self {
            Self::RestorePage { page, .. } | Self::RemovePage { page, .. } => {
                for entry in &page.nodes {
                    node_blobs(&entry.node, &mut ids);
                }
            }
            Self::RestoreSubtree { subtree, .. } | Self::RemoveSubtree { subtree, .. } => {
                for entry in &subtree.nodes {
                    node_blobs(&entry.node, &mut ids);
                }
            }
            Self::SetImage { from, to, .. } => ids.extend([from.blob, to.blob]),
            Self::SetMask { from, to, .. } => ids.extend([from.blob, to.blob]),
            Self::RenamePage { .. }
            | Self::ResizePage { .. }
            | Self::MovePage { .. }
            | Self::MoveSubtree { .. }
            | Self::SetName { .. }
            | Self::SetVisible { .. }
            | Self::SetOpacity { .. }
            | Self::SetTransform { .. }
            | Self::SetText { .. }
            | Self::SetTextStyle { .. }
            | Self::SetTextLayout { .. } => {}
        }
        ids
    }
}
