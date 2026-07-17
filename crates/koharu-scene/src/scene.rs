use std::collections::HashMap;

use indexmap::IndexMap;
use indextree::{Arena, NodeId as ArenaId};
use serde::{Deserialize, Serialize};

use crate::{
    CanvasSize, Error, ImageNode, MaskNode, Node, NodeId, NodeKind, PageId, Parent, Result,
    Revision, TextNode, Transform,
};

pub struct Scene {
    revision: Revision,
    pages: IndexMap<PageId, Page>,
    nodes: HashMap<NodeId, NodeLocation>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            revision: Revision::ZERO,
            pages: IndexMap::new(),
            nodes: HashMap::new(),
        }
    }
}

pub struct Page {
    id: PageId,
    name: String,
    size: CanvasSize,
    arena: Arena<TreeEntry>,
    root: ArenaId,
}

impl Page {
    #[must_use]
    pub fn new(name: impl Into<String>, size: CanvasSize) -> Self {
        let mut arena = Arena::new();
        let root = arena.new_node(TreeEntry::PageRoot);
        Self {
            id: PageId::new(),
            name: name.into(),
            size,
            arena,
            root,
        }
    }

    fn from_parts(id: PageId, name: String, size: CanvasSize) -> Self {
        let mut page = Self::new(name, size);
        page.id = id;
        page
    }

    #[must_use]
    pub const fn id(&self) -> PageId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn size(&self) -> CanvasSize {
        self.size
    }

    pub(crate) fn into_empty_snapshot(self) -> Result<PageSnapshot> {
        if self.root.children(&self.arena).next().is_some() {
            return Err(Error::invalid("a new Page must be empty"));
        }
        Ok(PageSnapshot {
            id: self.id,
            name: self.name,
            size: self.size,
            nodes: Vec::new(),
        })
    }
}

enum TreeEntry {
    PageRoot,
    Node(Node),
}

#[derive(Copy, Clone)]
struct NodeLocation {
    page: PageId,
    handle: ArenaId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum PlacementParent {
    Page(PageId),
    Node(NodeId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Placement {
    pub parent: PlacementParent,
    pub before: Option<NodeId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SceneSnapshot {
    pub revision: Revision,
    pub pages: Vec<PageSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct PageSnapshot {
    pub id: PageId,
    pub name: String,
    pub size: CanvasSize,
    pub nodes: Vec<SnapshotNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SubtreeSnapshot {
    pub nodes: Vec<SnapshotNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SnapshotNode {
    pub node: Node,
    pub parent: Option<NodeId>,
}

impl Scene {
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub fn page_ids(&self) -> impl DoubleEndedIterator<Item = PageId> + '_ {
        self.pages.keys().copied()
    }

    pub fn page(&self, id: PageId) -> Result<PageRef<'_>> {
        if self.pages.contains_key(&id) {
            Ok(PageRef { scene: self, id })
        } else {
            Err(Error::PageNotFound(id))
        }
    }

    pub fn node(&self, id: NodeId) -> Result<&Node> {
        let location = self.nodes.get(&id).ok_or(Error::NodeNotFound(id))?;
        self.node_at(*location)
    }

    pub fn page_of(&self, id: NodeId) -> Result<PageId> {
        self.node_page(id)
    }

    pub fn parent(&self, id: NodeId) -> Result<Parent> {
        Ok(match self.placement_of(id)?.parent {
            PlacementParent::Page(page) => Parent::Page(page),
            PlacementParent::Node(node) => Parent::Node(node),
        })
    }

    pub fn children(&self, parent: Parent) -> Result<Children<'_>> {
        let parent = match parent {
            Parent::Page(page) => PlacementParent::Page(page),
            Parent::Node(node) => PlacementParent::Node(node),
        };
        let (page_id, handle) = match parent {
            PlacementParent::Page(page) => (page, self.page_data(page)?.root),
            PlacementParent::Node(node) => {
                let location = *self.nodes.get(&node).ok_or(Error::NodeNotFound(node))?;
                let parent = self.node_at(location)?;
                if !parent.is_container() {
                    return Err(Error::WrongNodeKind {
                        node,
                        expected: "group or mask container",
                        actual: parent.kind.name(),
                    });
                }
                (location.page, location.handle)
            }
        };
        let page = self.page_data(page_id)?;
        Ok(Children {
            page,
            next: page.arena[handle].first_child(),
        })
    }

    #[must_use]
    pub fn contains_page(&self, id: PageId) -> bool {
        self.pages.contains_key(&id)
    }

    #[must_use]
    pub fn contains_node(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn page_data(&self, id: PageId) -> Result<&Page> {
        self.pages.get(&id).ok_or(Error::PageNotFound(id))
    }

    pub(crate) fn node_page(&self, id: NodeId) -> Result<PageId> {
        self.nodes
            .get(&id)
            .map(|location| location.page)
            .ok_or(Error::NodeNotFound(id))
    }

    fn node_at(&self, location: NodeLocation) -> Result<&Node> {
        let page = self.page_data(location.page)?;
        match page.arena[location.handle].get() {
            TreeEntry::Node(node) => Ok(node),
            TreeEntry::PageRoot => Err(Error::invalid("node index points at a page root")),
        }
    }

    fn node_at_mut(&mut self, id: NodeId) -> Result<&mut Node> {
        let location = *self.nodes.get(&id).ok_or(Error::NodeNotFound(id))?;
        let page = self
            .pages
            .get_mut(&location.page)
            .ok_or(Error::PageNotFound(location.page))?;
        match page.arena[location.handle].get_mut() {
            TreeEntry::Node(node) => Ok(node),
            TreeEntry::PageRoot => Err(Error::invalid("node index points at a page root")),
        }
    }

    pub(crate) fn set_revision(&mut self, revision: Revision) {
        self.revision = revision;
    }

    pub(crate) fn insert_page(&mut self, page: Page, index: usize) -> Result<()> {
        if self.pages.contains_key(&page.id) {
            return Err(Error::PageAlreadyExists(page.id));
        }
        if page.root.children(&page.arena).next().is_some() {
            return Err(Error::invalid("a newly inserted Page must be empty"));
        }
        let index = index.min(self.pages.len());
        self.pages.shift_insert(index, page.id, page);
        Ok(())
    }

    pub(crate) fn page_index(&self, id: PageId) -> Result<usize> {
        self.pages.get_index_of(&id).ok_or(Error::PageNotFound(id))
    }

    pub(crate) fn move_page(&mut self, id: PageId, to: usize) -> Result<usize> {
        let from = self.page_index(id)?;
        if to >= self.pages.len() {
            return Err(Error::invalid("page position is outside the project"));
        }
        self.pages.move_index(from, to);
        Ok(from)
    }

    pub(crate) fn snapshot_page(&self, id: PageId) -> Result<PageSnapshot> {
        let page = self.page_data(id)?;
        let mut nodes = Vec::new();
        for handle in page.root.descendants(&page.arena).skip(1) {
            let node = match page.arena[handle].get() {
                TreeEntry::Node(node) => node.clone(),
                TreeEntry::PageRoot => {
                    return Err(Error::invalid("nested page root in page arena"));
                }
            };
            let parent = handle
                .parent(&page.arena)
                .filter(|parent| *parent != page.root)
                .map(|parent| match page.arena[parent].get() {
                    TreeEntry::Node(node) => Ok(node.id),
                    TreeEntry::PageRoot => Err(Error::invalid("invalid subtree parent")),
                })
                .transpose()?;
            nodes.push(SnapshotNode { node, parent });
        }
        Ok(PageSnapshot {
            id: page.id,
            name: page.name.clone(),
            size: page.size,
            nodes,
        })
    }

    pub(crate) fn remove_page(&mut self, id: PageId) -> Result<(usize, PageSnapshot)> {
        let index = self.page_index(id)?;
        let snapshot = self.snapshot_page(id)?;
        let page = self
            .pages
            .shift_remove(&id)
            .ok_or(Error::PageNotFound(id))?;
        for handle in page.root.descendants(&page.arena).skip(1) {
            if let TreeEntry::Node(node) = page.arena[handle].get() {
                self.nodes.remove(&node.id);
            }
        }
        Ok((index, snapshot))
    }

    pub(crate) fn restore_page(&mut self, snapshot: &PageSnapshot, index: usize) -> Result<()> {
        if self.pages.contains_key(&snapshot.id) {
            return Err(Error::PageAlreadyExists(snapshot.id));
        }
        let page = Page::from_parts(snapshot.id, snapshot.name.clone(), snapshot.size);
        self.insert_page(page, index)?;
        for entry in &snapshot.nodes {
            let placement = Placement {
                parent: entry
                    .parent
                    .map_or(PlacementParent::Page(snapshot.id), PlacementParent::Node),
                before: None,
            };
            self.insert_node(entry.node.clone(), &placement)?;
        }
        Ok(())
    }

    pub(crate) fn rename_page(&mut self, id: PageId, name: String) -> Result<String> {
        let page = self.pages.get_mut(&id).ok_or(Error::PageNotFound(id))?;
        Ok(std::mem::replace(&mut page.name, name))
    }

    pub(crate) fn resize_page(&mut self, id: PageId, size: CanvasSize) -> Result<CanvasSize> {
        let page = self.pages.get_mut(&id).ok_or(Error::PageNotFound(id))?;
        Ok(std::mem::replace(&mut page.size, size))
    }

    fn placement_handles(
        &self,
        placement: &Placement,
    ) -> Result<(PageId, ArenaId, Option<ArenaId>)> {
        let (page_id, parent_handle) = match placement.parent {
            PlacementParent::Page(page) => {
                let page_data = self.page_data(page)?;
                (page, page_data.root)
            }
            PlacementParent::Node(parent) => {
                let location = *self.nodes.get(&parent).ok_or(Error::NodeNotFound(parent))?;
                let node = self.node_at(location)?;
                if !node.is_container() {
                    return Err(Error::WrongNodeKind {
                        node: parent,
                        expected: "group or mask container",
                        actual: node.kind.name(),
                    });
                }
                (location.page, location.handle)
            }
        };

        let before = placement
            .before
            .map(|id| {
                let location = *self.nodes.get(&id).ok_or(Error::NodeNotFound(id))?;
                if location.page != page_id {
                    return Err(Error::invalid("placement anchor is on another page"));
                }
                let page = self.page_data(page_id)?;
                if location.handle.parent(&page.arena) != Some(parent_handle) {
                    return Err(Error::invalid(
                        "placement anchor is outside the destination container",
                    ));
                }
                Ok(location.handle)
            })
            .transpose()?;
        Ok((page_id, parent_handle, before))
    }

    pub(crate) fn insert_node(&mut self, node: Node, placement: &Placement) -> Result<()> {
        if self.nodes.contains_key(&node.id) {
            return Err(Error::NodeAlreadyExists(node.id));
        }
        let (page_id, parent, before) = self.placement_handles(placement)?;
        let page = self
            .pages
            .get_mut(&page_id)
            .ok_or(Error::PageNotFound(page_id))?;
        let id = node.id;
        let handle = page.arena.new_node(TreeEntry::Node(node));
        if let Some(before) = before {
            before.insert_before(handle, &mut page.arena);
        } else {
            parent.append(handle, &mut page.arena);
        }
        self.nodes.insert(
            id,
            NodeLocation {
                page: page_id,
                handle,
            },
        );
        Ok(())
    }

    pub(crate) fn placement_of(&self, id: NodeId) -> Result<Placement> {
        let location = *self.nodes.get(&id).ok_or(Error::NodeNotFound(id))?;
        let page = self.page_data(location.page)?;
        let parent_handle = location
            .handle
            .parent(&page.arena)
            .ok_or_else(|| Error::invalid("node is detached"))?;
        let parent = if parent_handle == page.root {
            PlacementParent::Page(location.page)
        } else {
            match page.arena[parent_handle].get() {
                TreeEntry::Node(node) => PlacementParent::Node(node.id),
                TreeEntry::PageRoot => PlacementParent::Page(location.page),
            }
        };
        let before = page.arena[location.handle]
            .next_sibling()
            .map(|handle| match page.arena[handle].get() {
                TreeEntry::Node(node) => Ok(node.id),
                TreeEntry::PageRoot => Err(Error::invalid("page root is a sibling")),
            })
            .transpose()?;
        Ok(Placement { parent, before })
    }

    pub(crate) fn first_child(&self, parent: &PlacementParent) -> Result<Option<NodeId>> {
        let (page_id, handle) = match *parent {
            PlacementParent::Page(page) => (page, self.page_data(page)?.root),
            PlacementParent::Node(node) => {
                let location = *self.nodes.get(&node).ok_or(Error::NodeNotFound(node))?;
                let parent = self.node_at(location)?;
                if !parent.is_container() {
                    return Err(Error::WrongNodeKind {
                        node,
                        expected: "group or mask container",
                        actual: parent.kind.name(),
                    });
                }
                (location.page, location.handle)
            }
        };
        let page = self.page_data(page_id)?;
        handle
            .children(&page.arena)
            .next()
            .map(|child| match page.arena[child].get() {
                TreeEntry::Node(node) => Ok(node.id),
                TreeEntry::PageRoot => Err(Error::invalid("page root is a child")),
            })
            .transpose()
    }

    pub(crate) fn snapshot_subtree(&self, id: NodeId) -> Result<SubtreeSnapshot> {
        let location = *self.nodes.get(&id).ok_or(Error::NodeNotFound(id))?;
        let page = self.page_data(location.page)?;
        let mut nodes = Vec::new();
        for handle in location.handle.descendants(&page.arena) {
            let node = match page.arena[handle].get() {
                TreeEntry::Node(node) => node.clone(),
                TreeEntry::PageRoot => return Err(Error::invalid("page root inside subtree")),
            };
            let parent = if handle == location.handle {
                None
            } else {
                handle
                    .parent(&page.arena)
                    .map(|parent| match page.arena[parent].get() {
                        TreeEntry::Node(node) => Ok(node.id),
                        TreeEntry::PageRoot => Err(Error::invalid("invalid subtree parent")),
                    })
                    .transpose()?
            };
            nodes.push(SnapshotNode { node, parent });
        }
        Ok(SubtreeSnapshot { nodes })
    }

    pub(crate) fn remove_subtree(&mut self, id: NodeId) -> Result<(Placement, SubtreeSnapshot)> {
        let placement = self.placement_of(id)?;
        let snapshot = self.snapshot_subtree(id)?;
        let location = *self.nodes.get(&id).ok_or(Error::NodeNotFound(id))?;
        let page = self
            .pages
            .get_mut(&location.page)
            .ok_or(Error::PageNotFound(location.page))?;
        location.handle.remove_subtree(&mut page.arena);
        for entry in &snapshot.nodes {
            self.nodes.remove(&entry.node.id);
        }
        Ok((placement, snapshot))
    }

    pub(crate) fn restore_subtree(
        &mut self,
        snapshot: &SubtreeSnapshot,
        placement: &Placement,
    ) -> Result<()> {
        let root = snapshot
            .nodes
            .first()
            .ok_or_else(|| Error::invalid("cannot restore an empty subtree"))?;
        if root.parent.is_some() {
            return Err(Error::invalid("subtree root has an internal parent"));
        }
        self.insert_node(root.node.clone(), placement)?;
        for entry in snapshot.nodes.iter().skip(1) {
            let parent = entry
                .parent
                .ok_or_else(|| Error::invalid("subtree child is missing its parent"))?;
            self.insert_node(
                entry.node.clone(),
                &Placement {
                    parent: PlacementParent::Node(parent),
                    before: None,
                },
            )?;
        }
        Ok(())
    }

    pub(crate) fn move_subtree(&mut self, id: NodeId, placement: &Placement) -> Result<Placement> {
        let old = self.placement_of(id)?;
        if old == *placement {
            return Ok(old);
        }
        if placement.before == Some(id) {
            return Err(Error::invalid("a node cannot be placed before itself"));
        }

        let source = *self.nodes.get(&id).ok_or(Error::NodeNotFound(id))?;
        if let PlacementParent::Node(parent) = placement.parent {
            let node_page = source.page;
            let parent_page = self.node_page(parent)?;
            if node_page == parent_page {
                let node_location = self.nodes[&id];
                let parent_location = self.nodes[&parent];
                let page = self.page_data(node_page)?;
                if parent == id
                    || parent_location
                        .handle
                        .ancestors(&page.arena)
                        .any(|handle| handle == node_location.handle)
                {
                    return Err(Error::invalid("moving the node would create a cycle"));
                }
            }
        }

        let (destination_page, parent, before) = self.placement_handles(placement)?;
        if source.page == destination_page {
            let page = self
                .pages
                .get_mut(&source.page)
                .ok_or(Error::PageNotFound(source.page))?;
            source.handle.detach(&mut page.arena);
            if let Some(before) = before {
                before.insert_before(source.handle, &mut page.arena);
            } else {
                parent.append(source.handle, &mut page.arena);
            }
            return Ok(old);
        }

        let (_, snapshot) = self.remove_subtree(id)?;
        if let Err(error) = self.restore_subtree(&snapshot, placement) {
            self.restore_subtree(&snapshot, &old)
                .expect("restoring a validated source placement must succeed");
            return Err(error);
        }
        Ok(old)
    }

    pub(crate) fn set_name(&mut self, id: NodeId, value: Option<String>) -> Result<Option<String>> {
        Ok(std::mem::replace(&mut self.node_at_mut(id)?.name, value))
    }

    pub(crate) fn set_visible(&mut self, id: NodeId, value: bool) -> Result<bool> {
        Ok(std::mem::replace(&mut self.node_at_mut(id)?.visible, value))
    }

    pub(crate) fn set_opacity(&mut self, id: NodeId, value: f32) -> Result<f32> {
        Ok(std::mem::replace(&mut self.node_at_mut(id)?.opacity, value))
    }

    pub(crate) fn set_transform(&mut self, id: NodeId, value: Transform) -> Result<Transform> {
        Ok(std::mem::replace(
            &mut self.node_at_mut(id)?.transform,
            value,
        ))
    }

    pub(crate) fn set_image(&mut self, id: NodeId, value: ImageNode) -> Result<ImageNode> {
        match &mut self.node_at_mut(id)?.kind {
            NodeKind::Image(current) => Ok(std::mem::replace(current, value)),
            actual => Err(Error::WrongNodeKind {
                node: id,
                expected: "image",
                actual: actual.name(),
            }),
        }
    }

    pub(crate) fn set_mask(&mut self, id: NodeId, value: MaskNode) -> Result<MaskNode> {
        match &mut self.node_at_mut(id)?.kind {
            NodeKind::Mask(current) => Ok(std::mem::replace(current, value)),
            actual => Err(Error::WrongNodeKind {
                node: id,
                expected: "mask",
                actual: actual.name(),
            }),
        }
    }

    pub(crate) fn set_text(&mut self, id: NodeId, value: String) -> Result<String> {
        match &mut self.node_at_mut(id)?.kind {
            NodeKind::Text(current) => Ok(std::mem::replace(&mut current.text, value)),
            actual => Err(Error::WrongNodeKind {
                node: id,
                expected: "text",
                actual: actual.name(),
            }),
        }
    }

    pub(crate) fn set_text_style(
        &mut self,
        id: NodeId,
        value: crate::TextStyle,
    ) -> Result<crate::TextStyle> {
        match &mut self.node_at_mut(id)?.kind {
            NodeKind::Text(current) => Ok(std::mem::replace(&mut current.style, value)),
            actual => Err(Error::WrongNodeKind {
                node: id,
                expected: "text",
                actual: actual.name(),
            }),
        }
    }

    pub(crate) fn set_text_layout(
        &mut self,
        id: NodeId,
        value: crate::TextLayout,
    ) -> Result<crate::TextLayout> {
        match &mut self.node_at_mut(id)?.kind {
            NodeKind::Text(current) => Ok(std::mem::replace(&mut current.layout, value)),
            actual => Err(Error::WrongNodeKind {
                node: id,
                expected: "text",
                actual: actual.name(),
            }),
        }
    }

    pub(crate) fn to_snapshot(&self) -> Result<SceneSnapshot> {
        let pages = self
            .pages
            .keys()
            .copied()
            .map(|id| self.snapshot_page(id))
            .collect::<Result<Vec<_>>>()?;
        Ok(SceneSnapshot {
            revision: self.revision,
            pages,
        })
    }

    pub(crate) fn from_snapshot(snapshot: &SceneSnapshot) -> Result<Self> {
        let mut scene = Self::default();
        for (index, page) in snapshot.pages.iter().enumerate() {
            scene.restore_page(page, index)?;
        }
        scene.revision = snapshot.revision;
        Ok(scene)
    }

    pub(crate) fn blob_ids(&self) -> impl Iterator<Item = crate::BlobId> + '_ {
        self.nodes.values().filter_map(|location| {
            let node = self.node_at(*location).ok()?;
            match &node.kind {
                NodeKind::Image(image) => Some(image.blob),
                NodeKind::Mask(mask) => Some(mask.blob),
                NodeKind::Group | NodeKind::Text(_) => None,
            }
        })
    }
}

#[derive(Copy, Clone)]
pub struct PageRef<'a> {
    scene: &'a Scene,
    id: PageId,
}

impl<'a> PageRef<'a> {
    fn page(self) -> &'a Page {
        &self.scene.pages[&self.id]
    }

    #[must_use]
    pub const fn id(self) -> PageId {
        self.id
    }

    #[must_use]
    pub fn name(self) -> &'a str {
        self.page().name()
    }

    #[must_use]
    pub fn size(self) -> CanvasSize {
        self.page().size()
    }

    pub fn node(self, id: NodeId) -> Result<&'a Node> {
        if self.scene.node_page(id)? != self.id {
            return Err(Error::NodeNotFound(id));
        }
        self.scene.node(id)
    }

    pub fn image(self, id: NodeId) -> Result<&'a ImageNode> {
        let node = self.node(id)?;
        match &node.kind {
            NodeKind::Image(image) => Ok(image),
            kind => Err(Error::WrongNodeKind {
                node: id,
                expected: "image",
                actual: kind.name(),
            }),
        }
    }

    pub fn mask(self, id: NodeId) -> Result<&'a MaskNode> {
        let node = self.node(id)?;
        match &node.kind {
            NodeKind::Mask(mask) => Ok(mask),
            kind => Err(Error::WrongNodeKind {
                node: id,
                expected: "mask",
                actual: kind.name(),
            }),
        }
    }

    pub fn text(self, id: NodeId) -> Result<&'a TextNode> {
        let node = self.node(id)?;
        match &node.kind {
            NodeKind::Text(text) => Ok(text),
            kind => Err(Error::WrongNodeKind {
                node: id,
                expected: "text",
                actual: kind.name(),
            }),
        }
    }

    #[must_use]
    pub fn walk(self) -> Walk<'a> {
        Walk::new(self.page())
    }

    pub fn children(self) -> Result<Children<'a>> {
        self.scene.children(Parent::Page(self.id))
    }
}

pub struct Children<'a> {
    page: &'a Page,
    next: Option<ArenaId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let handle = self.next?;
        self.next = self.page.arena[handle].next_sibling();
        match self.page.arena[handle].get() {
            TreeEntry::Node(node) => Some(node.id),
            TreeEntry::PageRoot => {
                debug_assert!(false, "page root cannot be a child");
                self.next()
            }
        }
    }
}

#[derive(Copy, Clone)]
pub struct Visit<'a> {
    node: &'a Node,
    world_transform: Transform,
    effective_opacity: f32,
    effective_visibility: bool,
}

impl<'a> Visit<'a> {
    #[must_use]
    pub const fn node(self) -> &'a Node {
        self.node
    }

    #[must_use]
    pub const fn world_transform(self) -> Transform {
        self.world_transform
    }

    #[must_use]
    pub const fn effective_opacity(self) -> f32 {
        self.effective_opacity
    }

    #[must_use]
    pub const fn effective_visibility(self) -> bool {
        self.effective_visibility
    }
}

pub enum WalkEvent<'a> {
    Enter(Visit<'a>),
    Exit(NodeId),
}

enum WalkFrame {
    Enter(ArenaId, Transform, f32, bool),
    Exit(NodeId),
}

pub struct Walk<'a> {
    page: &'a Page,
    stack: Vec<WalkFrame>,
}

impl<'a> Walk<'a> {
    fn new(page: &'a Page) -> Self {
        let stack = page
            .root
            .children(&page.arena)
            .rev()
            .map(|handle| WalkFrame::Enter(handle, Transform::IDENTITY, 1.0, true))
            .collect();
        Self { page, stack }
    }
}

impl<'a> Iterator for Walk<'a> {
    type Item = WalkEvent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.stack.pop()? {
            WalkFrame::Exit(id) => Some(WalkEvent::Exit(id)),
            WalkFrame::Enter(handle, parent_transform, parent_opacity, parent_visibility) => {
                let node = match self.page.arena[handle].get() {
                    TreeEntry::Node(node) => node,
                    TreeEntry::PageRoot => return self.next(),
                };
                let world_transform = parent_transform.then(node.transform);
                let effective_opacity = parent_opacity * node.opacity;
                let effective_visibility = parent_visibility && node.visible;
                if node.is_container() {
                    self.stack.push(WalkFrame::Exit(node.id));
                    self.stack
                        .extend(handle.children(&self.page.arena).rev().map(|child| {
                            WalkFrame::Enter(
                                child,
                                world_transform,
                                effective_opacity,
                                effective_visibility,
                            )
                        }));
                }
                Some(WalkEvent::Enter(Visit {
                    node,
                    world_transform,
                    effective_opacity,
                    effective_visibility,
                }))
            }
        }
    }
}
