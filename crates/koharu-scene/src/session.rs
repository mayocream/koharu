use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::Path,
    sync::Arc,
    time::Duration,
};

use rusqlite::{MAIN_DB, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    BlobId, CanvasSize, CommandBatch, CommandId, Edit, Error, Node, NodeId, NodeKind, PageId,
    PagePosition, PixelSize, Result, Revision, Scene, TextEffectKind, TextNode, blob,
    command::{PendingOp, StoredBatch, StoredOp, resolve_placement},
    scene::{Placement, SceneSnapshot, SubtreeSnapshot},
    storage,
};

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub busy_timeout: Duration,
    /// Automatically checkpoint every N revisions. `None` or zero disables it.
    pub checkpoint_interval: Option<u64>,
    pub max_pages: usize,
    pub max_nodes: usize,
    pub max_commands_per_batch: usize,
    pub max_blob_bytes: usize,
    pub max_batch_attachment_bytes: usize,
    pub max_image_width: u32,
    pub max_image_height: u32,
    pub max_image_pixels: u64,
    pub max_text_bytes: usize,
    pub max_font_families: usize,
    pub max_text_effects: usize,
    pub max_gradient_stops_per_effect: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            busy_timeout: Duration::from_secs(2),
            checkpoint_interval: Some(1_024),
            max_pages: 10_000,
            max_nodes: 1_000_000,
            max_commands_per_batch: 100_000,
            max_blob_bytes: 256 * 1024 * 1024,
            max_batch_attachment_bytes: 512 * 1024 * 1024,
            max_image_width: 65_535,
            max_image_height: 65_535,
            max_image_pixels: 250_000_000,
            max_text_bytes: 16 * 1024 * 1024,
            max_font_families: 32,
            max_text_effects: 64,
            max_gradient_stops_per_effect: 256,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeChangeFlags(u16);

impl NodeChangeFlags {
    pub const NAME: Self = Self(1 << 0);
    pub const VISIBILITY: Self = Self(1 << 1);
    pub const OPACITY: Self = Self(1 << 2);
    pub const TRANSFORM: Self = Self(1 << 3);
    pub const CONTENT: Self = Self(1 << 4);
    pub const ORDER: Self = Self(1 << 5);

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangeSet {
    from_revision: Revision,
    to_revision: Revision,
    created_pages: Vec<PageId>,
    removed_pages: Vec<PageId>,
    updated_pages: Vec<PageId>,
    created_nodes: Vec<NodeId>,
    removed_nodes: Vec<NodeId>,
    moved_nodes: Vec<NodeId>,
    updated_nodes: BTreeMap<NodeId, NodeChangeFlags>,
    dirty_subtrees: Vec<NodeId>,
    reset: bool,
}

impl ChangeSet {
    fn empty(revision: Revision) -> Self {
        Self {
            from_revision: revision,
            to_revision: revision,
            created_pages: Vec::new(),
            removed_pages: Vec::new(),
            updated_pages: Vec::new(),
            created_nodes: Vec::new(),
            removed_nodes: Vec::new(),
            moved_nodes: Vec::new(),
            updated_nodes: BTreeMap::new(),
            dirty_subtrees: Vec::new(),
            reset: false,
        }
    }

    fn from_ops(from: Revision, to: Revision, ops: &[StoredOp]) -> Self {
        let mut changes = Self::empty(from);
        let mut seen = ChangeSetSeen::default();
        changes.to_revision = to;
        for op in ops {
            changes.record(op, &mut seen);
        }
        changes
    }

    fn record(&mut self, op: &StoredOp, seen: &mut ChangeSetSeen) {
        match op {
            StoredOp::RestorePage { page, .. } => {
                push_unique(&mut self.created_pages, &mut seen.created_pages, page.id);
                for entry in &page.nodes {
                    push_unique(
                        &mut self.created_nodes,
                        &mut seen.created_nodes,
                        entry.node.id(),
                    );
                }
            }
            StoredOp::RemovePage { page, .. } => {
                push_unique(&mut self.removed_pages, &mut seen.removed_pages, page.id);
                for entry in &page.nodes {
                    push_unique(
                        &mut self.removed_nodes,
                        &mut seen.removed_nodes,
                        entry.node.id(),
                    );
                }
            }
            StoredOp::RenamePage { page, .. }
            | StoredOp::ResizePage { page, .. }
            | StoredOp::MovePage { page, .. } => {
                push_unique(&mut self.updated_pages, &mut seen.updated_pages, *page);
            }
            StoredOp::RestoreSubtree { subtree, .. } => {
                for entry in &subtree.nodes {
                    push_unique(
                        &mut self.created_nodes,
                        &mut seen.created_nodes,
                        entry.node.id(),
                    );
                }
                if let Some(root) = subtree.nodes.first() {
                    push_unique(
                        &mut self.dirty_subtrees,
                        &mut seen.dirty_subtrees,
                        root.node.id(),
                    );
                }
            }
            StoredOp::RemoveSubtree { subtree, .. } => {
                for entry in &subtree.nodes {
                    push_unique(
                        &mut self.removed_nodes,
                        &mut seen.removed_nodes,
                        entry.node.id(),
                    );
                }
            }
            StoredOp::MoveSubtree { node, .. } => {
                push_unique(&mut self.moved_nodes, &mut seen.moved_nodes, *node);
                self.mark(*node, NodeChangeFlags::ORDER);
                push_unique(&mut self.dirty_subtrees, &mut seen.dirty_subtrees, *node);
            }
            StoredOp::SetName { node, .. } => self.mark(*node, NodeChangeFlags::NAME),
            StoredOp::SetVisible { node, .. } => {
                self.mark(*node, NodeChangeFlags::VISIBILITY);
                push_unique(&mut self.dirty_subtrees, &mut seen.dirty_subtrees, *node);
            }
            StoredOp::SetOpacity { node, .. } => {
                self.mark(*node, NodeChangeFlags::OPACITY);
                push_unique(&mut self.dirty_subtrees, &mut seen.dirty_subtrees, *node);
            }
            StoredOp::SetTransform { node, .. } => {
                self.mark(*node, NodeChangeFlags::TRANSFORM);
                push_unique(&mut self.dirty_subtrees, &mut seen.dirty_subtrees, *node);
            }
            StoredOp::SetImage { node, .. }
            | StoredOp::SetMask { node, .. }
            | StoredOp::SetText { node, .. }
            | StoredOp::SetTextStyle { node, .. }
            | StoredOp::SetTextLayout { node, .. } => {
                self.mark(*node, NodeChangeFlags::CONTENT);
                push_unique(&mut self.dirty_subtrees, &mut seen.dirty_subtrees, *node);
            }
        }
    }

    fn mark(&mut self, node: NodeId, flag: NodeChangeFlags) {
        self.updated_nodes.entry(node).or_default().insert(flag);
    }

    fn merge(&mut self, other: Self, seen: &mut ChangeSetSeen) {
        self.to_revision = other.to_revision;
        self.reset |= other.reset;
        extend_unique(
            &mut self.created_pages,
            &mut seen.created_pages,
            other.created_pages,
        );
        extend_unique(
            &mut self.removed_pages,
            &mut seen.removed_pages,
            other.removed_pages,
        );
        extend_unique(
            &mut self.updated_pages,
            &mut seen.updated_pages,
            other.updated_pages,
        );
        extend_unique(
            &mut self.created_nodes,
            &mut seen.created_nodes,
            other.created_nodes,
        );
        extend_unique(
            &mut self.removed_nodes,
            &mut seen.removed_nodes,
            other.removed_nodes,
        );
        extend_unique(
            &mut self.moved_nodes,
            &mut seen.moved_nodes,
            other.moved_nodes,
        );
        extend_unique(
            &mut self.dirty_subtrees,
            &mut seen.dirty_subtrees,
            other.dirty_subtrees,
        );
        for (node, flags) in other.updated_nodes {
            self.updated_nodes.entry(node).or_default().insert(flags);
        }
    }

    #[must_use]
    pub const fn from_revision(&self) -> Revision {
        self.from_revision
    }

    #[must_use]
    pub const fn to_revision(&self) -> Revision {
        self.to_revision
    }

    #[must_use]
    pub fn created_pages(&self) -> &[PageId] {
        &self.created_pages
    }

    #[must_use]
    pub fn removed_pages(&self) -> &[PageId] {
        &self.removed_pages
    }

    #[must_use]
    pub fn updated_pages(&self) -> &[PageId] {
        &self.updated_pages
    }

    #[must_use]
    pub fn created_nodes(&self) -> &[NodeId] {
        &self.created_nodes
    }

    #[must_use]
    pub fn removed_nodes(&self) -> &[NodeId] {
        &self.removed_nodes
    }

    #[must_use]
    pub fn moved_nodes(&self) -> &[NodeId] {
        &self.moved_nodes
    }

    #[must_use]
    pub fn node_changes(&self, node: NodeId) -> NodeChangeFlags {
        self.updated_nodes.get(&node).copied().unwrap_or_default()
    }

    #[must_use]
    pub fn dirty_subtrees(&self) -> &[NodeId] {
        &self.dirty_subtrees
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.reset
            && self.created_pages.is_empty()
            && self.removed_pages.is_empty()
            && self.updated_pages.is_empty()
            && self.created_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.moved_nodes.is_empty()
            && self.updated_nodes.is_empty()
            && self.dirty_subtrees.is_empty()
    }

    #[must_use]
    pub const fn requires_reload(&self) -> bool {
        self.reset
    }
}

#[derive(Default)]
struct ChangeSetSeen {
    created_pages: HashSet<PageId>,
    removed_pages: HashSet<PageId>,
    updated_pages: HashSet<PageId>,
    created_nodes: HashSet<NodeId>,
    removed_nodes: HashSet<NodeId>,
    moved_nodes: HashSet<NodeId>,
    dirty_subtrees: HashSet<NodeId>,
}

fn push_unique<T: Eq + std::hash::Hash + Copy>(
    values: &mut Vec<T>,
    seen: &mut HashSet<T>,
    value: T,
) {
    if seen.insert(value) {
        values.push(value);
    }
}

fn extend_unique<T: Eq + std::hash::Hash + Copy>(
    values: &mut Vec<T>,
    seen: &mut HashSet<T>,
    additions: Vec<T>,
) {
    for value in additions {
        push_unique(values, seen, value);
    }
}

#[derive(Clone, Debug)]
pub struct Applied {
    pub revision: Revision,
    pub changes: ChangeSet,
    pub already_applied: bool,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct GcReport {
    pub commits_deleted: u64,
    pub blobs_deleted: u64,
    pub blob_bytes_deleted: u64,
}

pub struct Session {
    connection: rusqlite::Connection,
    scene: Scene,
    config: SessionConfig,
    project_id: Uuid,
    poisoned: bool,
}

struct Prepared {
    forward: StoredBatch,
    changes: ChangeSet,
    blob_refs: Vec<BlobId>,
    checkpoint: Option<Vec<u8>>,
}

struct Receipt {
    revision: Revision,
    request_hash: [u8; 32],
    changes: ChangeSet,
}

struct CommitRecord {
    revision: Revision,
    parent: Revision,
    forward: StoredBatch,
    changes: ChangeSet,
}

impl Session {
    pub fn create(path: impl AsRef<Path>, config: SessionConfig) -> Result<Self> {
        let connection = storage::create_disk(path.as_ref(), config.busy_timeout)?;
        let has_project = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'project'",
                [],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if has_project {
            return Err(Error::invalid("project database already exists"));
        }
        let project_id = Uuid::now_v7();
        storage::create_schema(&connection, project_id)?;
        Ok(Self {
            connection,
            scene: Scene::default(),
            config,
            project_id,
            poisoned: false,
        })
    }

    pub fn open(path: impl AsRef<Path>, config: SessionConfig) -> Result<Self> {
        let connection = storage::open_disk(path.as_ref(), config.busy_timeout)?;
        Self::from_connection(connection, config)
    }

    pub fn memory(config: SessionConfig) -> Result<Self> {
        let connection = storage::open_memory(config.busy_timeout)?;
        let project_id = Uuid::now_v7();
        storage::create_schema(&connection, project_id)?;
        Ok(Self {
            connection,
            scene: Scene::default(),
            config,
            project_id,
            poisoned: false,
        })
    }

    fn from_connection(connection: rusqlite::Connection, config: SessionConfig) -> Result<Self> {
        let project = storage::project(&connection)?;
        let scene = load_scene(&connection, &project)?;
        verify_scene_blobs(&connection, &scene)?;
        Ok(Self {
            connection,
            scene,
            config,
            project_id: project.project_id,
            poisoned: false,
        })
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.scene.revision()
    }

    #[must_use]
    pub const fn scene(&self) -> &Scene {
        &self.scene
    }

    #[must_use]
    pub const fn project_id(&self) -> Uuid {
        self.project_id
    }

    pub fn blob(&self, id: BlobId) -> Result<Arc<[u8]>> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT bytes FROM blobs WHERE id = ?1")?;
        let bytes = statement
            .query_row([id.as_bytes().as_slice()], |row| row.get::<_, Vec<u8>>(0))
            .optional()?
            .ok_or_else(|| Error::invalid(format!("blob {id} does not exist")))?;
        Ok(bytes.into())
    }

    pub fn edit(&mut self) -> Edit<'_> {
        Edit::new(self)
    }

    pub fn apply(&mut self, batch: CommandBatch) -> Result<Applied> {
        self.ensure_usable()?;
        let durable = storage::project(&self.connection)?.head;
        if durable != self.revision() {
            return Err(Error::StaleSession {
                local: self.revision(),
                durable,
            });
        }

        let request_hash = batch.request_hash()?;
        if let Some(receipt) = receipt(&self.connection, batch.id)? {
            return receipt_result(receipt, request_hash);
        }
        if batch.base_revision != self.revision() {
            return Err(Error::RevisionConflict {
                base: batch.base_revision,
                current: self.revision(),
            });
        }
        self.validate_batch_limits(&batch)?;
        let CommandBatch {
            id,
            attachments,
            ops,
            ..
        } = batch;
        let prepared = self.prepare_pending(ops, &attachments)?;
        self.commit_prepared(id, request_hash, &attachments, prepared)
    }

    fn ensure_usable(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Poisoned)
        } else {
            Ok(())
        }
    }

    fn validate_batch_limits(&self, batch: &CommandBatch) -> Result<()> {
        if batch.ops.len() > self.config.max_commands_per_batch {
            return Err(Error::invalid("command count exceeds configured limit"));
        }
        let mut total = 0usize;
        for (id, bytes) in &batch.attachments {
            if bytes.len() > self.config.max_blob_bytes {
                return Err(Error::invalid(format!(
                    "blob {id} exceeds configured byte limit"
                )));
            }
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| Error::invalid("attachment byte count overflow"))?;
        }
        if total > self.config.max_batch_attachment_bytes {
            return Err(Error::invalid(
                "batch attachments exceed configured byte limit",
            ));
        }
        Ok(())
    }

    fn prepare_pending(
        &mut self,
        pending_ops: Vec<PendingOp>,
        attachments: &HashMap<BlobId, Arc<[u8]>>,
    ) -> Result<Prepared> {
        let next = self
            .revision()
            .next()
            .ok_or_else(|| Error::invalid("revision overflow"))?;
        let mut forward = Vec::new();
        let mut metadata = HashMap::new();
        let result = (|| {
            for pending in pending_ops {
                if let Some(op) = self.materialize_pending(attachments, pending, &mut metadata)? {
                    op.apply(&mut self.scene)?;
                    forward.push(op);
                }
            }
            self.validate_scene_limits()?;
            Ok(())
        })();

        if let Err(error) = result {
            rollback(&mut self.scene, &forward);
            return Err(error);
        }

        let stored = StoredBatch::new(forward);
        let changes = ChangeSet::from_ops(self.revision(), next, &stored.ops);
        let checkpoint = match self.automatic_checkpoint(next) {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                rollback(&mut self.scene, &stored.ops);
                return Err(error);
            }
        };
        let mut blob_refs = stored.blob_ids();
        if checkpoint.is_some() {
            blob_refs.extend(self.scene.blob_ids());
            blob_refs.sort_unstable();
            blob_refs.dedup();
        }
        for op in stored.ops.iter().rev() {
            op.apply_backward(&mut self.scene)
                .expect("generated inverse must restore the validated scene");
        }

        Ok(Prepared {
            forward: stored,
            changes,
            blob_refs,
            checkpoint,
        })
    }

    fn materialize_pending(
        &self,
        attachments: &HashMap<BlobId, Arc<[u8]>>,
        pending: PendingOp,
        metadata: &mut HashMap<(BlobId, bool), PixelSize>,
    ) -> Result<Option<StoredOp>> {
        let op = match pending {
            PendingOp::CreatePage { page } => {
                validate_canvas(page.size)?;
                StoredOp::RestorePage {
                    page,
                    index: self.scene.page_count(),
                }
            }
            PendingOp::RemovePage { page } => StoredOp::RemovePage {
                page: self.scene.snapshot_page(page)?,
                index: self.scene.page_index(page)?,
            },
            PendingOp::MovePage { page, position } => {
                let from = self.scene.page_index(page)?;
                let to = match position {
                    PagePosition::First => 0,
                    PagePosition::Last => self.scene.page_count() - 1,
                    PagePosition::Before(anchor) => {
                        if page == anchor {
                            return Ok(None);
                        }
                        let anchor = self.scene.page_index(anchor)?;
                        if from < anchor { anchor - 1 } else { anchor }
                    }
                    PagePosition::After(anchor) => {
                        if page == anchor {
                            return Ok(None);
                        }
                        let anchor = self.scene.page_index(anchor)?;
                        if from < anchor { anchor } else { anchor + 1 }
                    }
                };
                if from == to {
                    return Ok(None);
                }
                StoredOp::MovePage { page, from, to }
            }
            PendingOp::RenamePage { page, name } => {
                let from = self.scene.page_data(page)?.name().to_owned();
                if from == name {
                    return Ok(None);
                }
                StoredOp::RenamePage {
                    page,
                    from,
                    to: name,
                }
            }
            PendingOp::ResizePage { page, size } => {
                validate_canvas(size)?;
                let from = self.scene.page_data(page)?.size();
                if from == size {
                    return Ok(None);
                }
                StoredOp::ResizePage {
                    page,
                    from,
                    to: size,
                }
            }
            PendingOp::CreateNode {
                parent,
                position,
                node,
            } => {
                let placement = resolve_placement(&self.scene, parent, position)?;
                let node = node.into_node(|blob, mask| {
                    self.inspect_attachment(attachments, blob, mask, metadata)
                })?;
                self.validate_node(&node)?;
                StoredOp::RestoreSubtree {
                    subtree: SubtreeSnapshot {
                        nodes: vec![crate::scene::SnapshotNode { node, parent: None }],
                    },
                    placement,
                }
            }
            PendingOp::RemoveNode { node } => {
                let placement = self.scene.placement_of(node)?;
                let subtree = self.scene.snapshot_subtree(node)?;
                StoredOp::RemoveSubtree { subtree, placement }
            }
            PendingOp::MoveNode {
                node,
                parent,
                position,
            } => {
                let from = self.scene.placement_of(node)?;
                let to = resolve_placement(&self.scene, parent, position)?;
                if from == to || to.before == Some(node) {
                    return Ok(None);
                }
                StoredOp::MoveSubtree { node, from, to }
            }
            PendingOp::PlaceRelative {
                node,
                anchor: anchor_id,
                above,
            } => {
                if node == anchor_id {
                    return Err(Error::invalid("a node cannot be placed relative to itself"));
                }
                let from = self.scene.placement_of(node)?;
                let anchor = self.scene.placement_of(anchor_id)?;
                let to = Placement {
                    parent: anchor.parent,
                    before: if above {
                        anchor.before
                    } else {
                        Some(anchor_id)
                    },
                };
                if from == to || to.before == Some(node) {
                    return Ok(None);
                }
                StoredOp::MoveSubtree { node, from, to }
            }
            PendingOp::SetName { node, name } => {
                let from = self.scene.node(node)?.name().map(ToOwned::to_owned);
                if from == name {
                    return Ok(None);
                }
                StoredOp::SetName {
                    node,
                    from,
                    to: name,
                }
            }
            PendingOp::SetVisible { node, visible } => {
                let from = self.scene.node(node)?.visible();
                if from == visible {
                    return Ok(None);
                }
                StoredOp::SetVisible {
                    node,
                    from,
                    to: visible,
                }
            }
            PendingOp::SetOpacity { node, opacity } => {
                validate_opacity(opacity)?;
                let from = self.scene.node(node)?.opacity();
                if from == opacity {
                    return Ok(None);
                }
                StoredOp::SetOpacity {
                    node,
                    from,
                    to: opacity,
                }
            }
            PendingOp::SetTransform { node, transform } => {
                if !transform.is_finite() {
                    return Err(Error::invalid("transform values must be finite"));
                }
                let from = self.scene.node(node)?.transform();
                if from == transform {
                    return Ok(None);
                }
                StoredOp::SetTransform {
                    node,
                    from,
                    to: transform,
                }
            }
            PendingOp::SetImage { node, blob } => {
                let size = self.inspect_attachment(attachments, blob, false, metadata)?;
                let NodeKind::Image(from) = self.scene.node(node)?.kind() else {
                    return Err(wrong_kind(node, "image", self.scene.node(node)?.kind()));
                };
                let to = crate::ImageNode {
                    blob,
                    natural_size: size,
                };
                if *from == to {
                    return Ok(None);
                }
                StoredOp::SetImage {
                    node,
                    from: from.clone(),
                    to,
                }
            }
            PendingOp::SetMask { node, blob } => {
                let size = self.inspect_attachment(attachments, blob, true, metadata)?;
                let NodeKind::Mask(from) = self.scene.node(node)?.kind() else {
                    return Err(wrong_kind(node, "mask", self.scene.node(node)?.kind()));
                };
                let to = crate::MaskNode {
                    blob,
                    natural_size: size,
                };
                if *from == to {
                    return Ok(None);
                }
                StoredOp::SetMask {
                    node,
                    from: from.clone(),
                    to,
                }
            }
            PendingOp::SetText { node, text } => {
                let NodeKind::Text(value) = self.scene.node(node)?.kind() else {
                    return Err(wrong_kind(node, "text", self.scene.node(node)?.kind()));
                };
                if value.text == text {
                    return Ok(None);
                }
                if text.len() > self.config.max_text_bytes {
                    return Err(Error::invalid("text exceeds configured byte limit"));
                }
                StoredOp::SetText {
                    node,
                    from: value.text.clone(),
                    to: text,
                }
            }
            PendingOp::SetTextStyle { node, style } => {
                let NodeKind::Text(value) = self.scene.node(node)?.kind() else {
                    return Err(wrong_kind(node, "text", self.scene.node(node)?.kind()));
                };
                if value.style == style {
                    return Ok(None);
                }
                self.validate_text_style(&style)?;
                StoredOp::SetTextStyle {
                    node,
                    from: value.style.clone(),
                    to: style,
                }
            }
            PendingOp::SetTextLayout { node, layout } => {
                let NodeKind::Text(value) = self.scene.node(node)?.kind() else {
                    return Err(wrong_kind(node, "text", self.scene.node(node)?.kind()));
                };
                if value.layout == layout {
                    return Ok(None);
                }
                if !layout.is_valid() {
                    return Err(Error::invalid("text layout contains invalid values"));
                }
                StoredOp::SetTextLayout {
                    node,
                    from: value.layout.clone(),
                    to: layout,
                }
            }
        };
        Ok(Some(op))
    }

    fn inspect_attachment(
        &self,
        attachments: &HashMap<BlobId, Arc<[u8]>>,
        id: BlobId,
        mask: bool,
        metadata: &mut HashMap<(BlobId, bool), PixelSize>,
    ) -> Result<PixelSize> {
        if let Some(size) = metadata.get(&(id, mask)) {
            return Ok(*size);
        }
        let bytes = attachments
            .get(&id)
            .ok_or_else(|| Error::invalid(format!("blob {id} is not attached")))?;
        let size = blob::inspect(
            bytes,
            mask,
            self.config.max_image_width,
            self.config.max_image_height,
            self.config.max_image_pixels,
        )?;
        metadata.insert((id, mask), size);
        Ok(size)
    }

    fn validate_node(&self, node: &Node) -> Result<()> {
        if !node.transform().is_finite() {
            return Err(Error::invalid("transform values must be finite"));
        }
        validate_opacity(node.opacity())?;
        if node
            .name()
            .is_some_and(|name| name.len() > self.config.max_text_bytes)
        {
            return Err(Error::invalid("node name exceeds configured byte limit"));
        }
        match node.kind() {
            NodeKind::Text(text) => self.validate_text(text),
            NodeKind::Group | NodeKind::Mask(_) | NodeKind::Image(_) => Ok(()),
        }
    }

    fn validate_text(&self, text: &TextNode) -> Result<()> {
        if text.text.len() > self.config.max_text_bytes {
            return Err(Error::invalid("text exceeds configured byte limit"));
        }
        self.validate_text_style(&text.style)?;
        if !text.layout.is_valid() {
            return Err(Error::invalid("text layout contains invalid values"));
        }
        Ok(())
    }

    fn validate_text_style(&self, style: &crate::TextStyle) -> Result<()> {
        if style.font_families.len() > self.config.max_font_families {
            return Err(Error::invalid("font family count exceeds configured limit"));
        }
        let family_bytes = style
            .font_families
            .iter()
            .try_fold(0usize, |total, family| total.checked_add(family.len()))
            .ok_or_else(|| Error::invalid("font family byte count overflow"))?;
        if family_bytes > self.config.max_text_bytes {
            return Err(Error::invalid("font families exceed configured byte limit"));
        }
        if style.effects.len() > self.config.max_text_effects {
            return Err(Error::invalid("text effect count exceeds configured limit"));
        }
        if style.effects.iter().any(|effect| {
            matches!(
                &effect.kind,
                TextEffectKind::GradientOverlay { stops, .. }
                    if stops.len() > self.config.max_gradient_stops_per_effect
            )
        }) {
            return Err(Error::invalid(
                "gradient stop count exceeds configured limit",
            ));
        }
        if !style.is_valid() {
            return Err(Error::invalid("text style contains invalid values"));
        }
        Ok(())
    }

    fn validate_scene_limits(&self) -> Result<()> {
        if self.scene.page_count() > self.config.max_pages {
            return Err(Error::invalid("page count exceeds configured limit"));
        }
        if self.scene.node_count() > self.config.max_nodes {
            return Err(Error::invalid("node count exceeds configured limit"));
        }
        Ok(())
    }

    fn automatic_checkpoint(&self, revision: Revision) -> Result<Option<Vec<u8>>> {
        let Some(interval) = self
            .config
            .checkpoint_interval
            .filter(|interval| *interval > 0)
        else {
            return Ok(None);
        };
        if !revision.get().is_multiple_of(interval) {
            return Ok(None);
        }
        let mut snapshot = self.scene.to_snapshot()?;
        snapshot.revision = revision;
        Ok(Some(postcard::to_stdvec(&snapshot)?))
    }
}

impl Session {
    fn commit_prepared(
        &mut self,
        command_id: CommandId,
        request_hash: [u8; 32],
        attachments: &HashMap<BlobId, Arc<[u8]>>,
        prepared: Prepared,
    ) -> Result<Applied> {
        let base = self.revision();
        let next = prepared.changes.to_revision;
        let forward_bytes = postcard::to_stdvec(&prepared.forward)?;
        let blob_ref_bytes = postcard::to_stdvec(&prepared.blob_refs)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        if let Some(existing) = receipt(&transaction, command_id)? {
            let result = receipt_result(existing, request_hash);
            transaction.rollback()?;
            let applied = result?;
            self.refresh()?;
            return Ok(applied);
        }

        let project = storage::project(&transaction)?;
        if project.head != base {
            transaction.rollback()?;
            return Err(Error::RevisionConflict {
                base,
                current: project.head,
            });
        }

        {
            let mut insert_blob = transaction.prepare_cached(
                "INSERT INTO blobs (id, bytes) VALUES (?1, ?2)
                 ON CONFLICT(id) DO NOTHING",
            )?;
            for id in &prepared.blob_refs {
                if let Some(bytes) = attachments.get(id) {
                    insert_blob.execute(params![id.as_bytes().as_slice(), bytes.as_ref()])?;
                }
            }
        }
        verify_blob_ids(&transaction, prepared.blob_refs.iter().copied())?;

        if prepared.checkpoint.is_some()
            && let Some(old) = project.checkpoint
        {
            let refs = stored_batch(&transaction, old)?.blob_ids();
            transaction.execute(
                "UPDATE commits SET checkpoint = NULL, blob_refs = ?1 WHERE revision = ?2",
                params![postcard::to_stdvec(&refs)?, storage::revision_to_sql(old)?],
            )?;
        }

        transaction.execute(
            "INSERT INTO commits (
                revision, parent_revision, command_id, command_hash,
                forward_batch, blob_refs, checkpoint
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                storage::revision_to_sql(next)?,
                storage::revision_to_sql(base)?,
                command_id.as_uuid().as_bytes().as_slice(),
                request_hash.as_slice(),
                forward_bytes,
                blob_ref_bytes,
                prepared.checkpoint.as_deref(),
            ],
        )?;
        if prepared.checkpoint.is_some() {
            transaction.execute(
                "UPDATE project
                 SET head_revision = ?1, checkpoint_revision = ?1
                 WHERE singleton = 1",
                [storage::revision_to_sql(next)?],
            )?;
        } else {
            transaction.execute(
                "UPDATE project SET head_revision = ?1 WHERE singleton = 1",
                [storage::revision_to_sql(next)?],
            )?;
        }
        transaction.commit()?;

        for op in &prepared.forward.ops {
            if op.apply(&mut self.scene).is_err() {
                self.poisoned = true;
                return Err(Error::Poisoned);
            }
        }
        self.scene.set_revision(next);
        Ok(Applied {
            revision: next,
            changes: prepared.changes,
            already_applied: false,
        })
    }

    pub fn refresh(&mut self) -> Result<ChangeSet> {
        self.ensure_usable()?;
        let project = storage::project(&self.connection)?;
        let from = self.revision();
        if project.head == from {
            return Ok(ChangeSet::empty(from));
        }
        if project.head < from {
            return Err(Error::NotAProject);
        }

        if project
            .checkpoint
            .is_some_and(|checkpoint| checkpoint > from)
        {
            let scene = load_scene(&self.connection, &project)?;
            verify_scene_blobs(&self.connection, &scene)?;
            let mut changes = ChangeSet::empty(from);
            changes.to_revision = project.head;
            changes.reset = true;
            self.scene = scene;
            return Ok(changes);
        }

        let records = load_records(&self.connection, from, project.head)?;
        if records.first().is_none_or(|record| record.parent != from)
            || records
                .last()
                .is_none_or(|record| record.revision != project.head)
        {
            let scene = load_scene(&self.connection, &project)?;
            verify_scene_blobs(&self.connection, &scene)?;
            let mut changes = ChangeSet::empty(from);
            changes.to_revision = project.head;
            changes.reset = true;
            self.scene = scene;
            return Ok(changes);
        }

        let mut merged = ChangeSet::empty(from);
        let mut seen = ChangeSetSeen::default();
        let mut imported_blobs = BTreeSet::new();
        let mut applied = Vec::new();
        for record in records {
            if self.revision() != record.parent {
                rollback_batches(&mut self.scene, &applied);
                self.scene.set_revision(from);
                return Err(Error::NotAProject);
            }
            for op in &record.forward.ops {
                if let Err(error) = op.apply(&mut self.scene) {
                    rollback_batches(&mut self.scene, &applied);
                    self.scene.set_revision(from);
                    return Err(error);
                }
            }
            self.scene.set_revision(record.revision);
            imported_blobs.extend(record.forward.blob_ids());
            merged.merge(record.changes, &mut seen);
            applied.push(record.forward);
        }
        if let Err(error) = verify_blob_ids(&self.connection, imported_blobs) {
            rollback_batches(&mut self.scene, &applied);
            self.scene.set_revision(from);
            return Err(error);
        }
        Ok(merged)
    }

    pub fn revert(&mut self, revisions: &[Revision]) -> Result<Applied> {
        self.ensure_usable()?;
        if revisions.is_empty() {
            return Err(Error::invalid("revert requires at least one revision"));
        }
        for pair in revisions.windows(2) {
            if pair[0] <= pair[1] {
                return Err(Error::invalid(
                    "revisions must be unique and ordered newest-to-oldest",
                ));
            }
        }
        let durable = storage::project(&self.connection)?.head;
        if durable != self.revision() {
            return Err(Error::StaleSession {
                local: self.revision(),
                durable,
            });
        }

        let mut ops = Vec::new();
        for revision in revisions {
            let bytes = self
                .connection
                .query_row(
                    "SELECT forward_batch FROM commits WHERE revision = ?1",
                    [storage::revision_to_sql(*revision)?],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()?
                .ok_or(Error::RevisionNotRetained(*revision))?;
            let forward: StoredBatch = postcard::from_bytes(&bytes)?;
            validate_stored_version(&forward)?;
            ops.extend(forward.into_inverse().ops);
        }

        let command_id = CommandId::new();
        let request = postcard::to_stdvec(&(self.revision(), revisions))?;
        let request_hash = *blake3::hash(&request).as_bytes();
        let prepared = self.prepare_stored(StoredBatch::new(ops))?;
        self.commit_prepared(command_id, request_hash, &HashMap::new(), prepared)
    }

    fn prepare_stored(&mut self, forward: StoredBatch) -> Result<Prepared> {
        let next = self
            .revision()
            .next()
            .ok_or_else(|| Error::invalid("revision overflow"))?;
        for (applied, op) in forward.ops.iter().enumerate() {
            if let Err(error) = op.apply(&mut self.scene) {
                rollback(&mut self.scene, &forward.ops[..applied]);
                return Err(error);
            }
        }
        if let Err(error) = self.validate_scene_limits() {
            rollback(&mut self.scene, &forward.ops);
            return Err(error);
        }
        let changes = ChangeSet::from_ops(self.revision(), next, &forward.ops);
        let checkpoint = match self.automatic_checkpoint(next) {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                rollback(&mut self.scene, &forward.ops);
                return Err(error);
            }
        };
        let mut blob_refs = forward.blob_ids();
        if checkpoint.is_some() {
            blob_refs.extend(self.scene.blob_ids());
            blob_refs.sort_unstable();
            blob_refs.dedup();
        }
        for op in forward.ops.iter().rev() {
            op.apply_backward(&mut self.scene)
                .expect("validated history operation must be reversible");
        }
        Ok(Prepared {
            forward,
            changes,
            blob_refs,
            checkpoint,
        })
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        self.ensure_usable()?;
        let revision = self.revision();
        if revision == Revision::ZERO {
            return Ok(());
        }
        let project = storage::project(&self.connection)?;
        if project.head != revision {
            return Err(Error::StaleSession {
                local: revision,
                durable: project.head,
            });
        }
        if project.checkpoint == Some(revision) {
            return Ok(());
        }
        let snapshot = postcard::to_stdvec(&self.scene.to_snapshot()?)?;
        let scene_refs = self.scene.blob_ids().collect::<Vec<_>>();

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = storage::project(&transaction)?;
        if current.head != revision {
            transaction.rollback()?;
            return Err(Error::RevisionConflict {
                base: revision,
                current: current.head,
            });
        }

        if let Some(old) = current.checkpoint
            && old != revision
        {
            let refs = stored_batch(&transaction, old)?.blob_ids();
            transaction.execute(
                "UPDATE commits SET checkpoint = NULL, blob_refs = ?1 WHERE revision = ?2",
                params![postcard::to_stdvec(&refs)?, storage::revision_to_sql(old)?],
            )?;
        }

        let mut refs = stored_batch(&transaction, revision)?.blob_ids();
        refs.extend(scene_refs);
        refs.sort_unstable();
        refs.dedup();
        transaction.execute(
            "UPDATE commits SET checkpoint = ?1, blob_refs = ?2 WHERE revision = ?3",
            params![
                snapshot,
                postcard::to_stdvec(&refs)?,
                storage::revision_to_sql(revision)?
            ],
        )?;
        transaction.execute(
            "UPDATE project SET checkpoint_revision = ?1 WHERE singleton = 1",
            [storage::revision_to_sql(revision)?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn prune_history(&mut self, keep_from: Revision) -> Result<GcReport> {
        self.ensure_usable()?;
        let revision = self.revision();
        if keep_from > revision {
            return Err(Error::invalid("history retention starts after the head"));
        }
        self.checkpoint()?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let project = storage::project(&transaction)?;
        if project.head != revision {
            transaction.rollback()?;
            return Err(Error::RevisionConflict {
                base: revision,
                current: project.head,
            });
        }

        let commits_deleted = transaction.execute(
            "DELETE FROM commits WHERE revision < ?1 AND revision <> ?2",
            params![
                storage::revision_to_sql(keep_from)?,
                storage::revision_to_sql(revision)?
            ],
        )? as u64;

        let mut live = self.scene.blob_ids().collect::<BTreeSet<_>>();
        {
            let mut statement = transaction.prepare("SELECT blob_refs FROM commits")?;
            let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
            for row in rows {
                let refs: Vec<BlobId> = postcard::from_bytes(&row?)?;
                live.extend(refs);
            }
        }

        transaction.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS koharu_live_blobs (
                 id BLOB PRIMARY KEY NOT NULL
             ) WITHOUT ROWID;
             DELETE FROM koharu_live_blobs;",
        )?;
        {
            let mut insert =
                transaction.prepare_cached("INSERT INTO koharu_live_blobs (id) VALUES (?1)")?;
            for id in &live {
                insert.execute([id.as_bytes().as_slice()])?;
            }
        }
        let (blobs_deleted, blob_bytes_deleted) = transaction.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(bytes)), 0)
             FROM blobs
             WHERE NOT EXISTS (
                 SELECT 1 FROM koharu_live_blobs live WHERE live.id = blobs.id
             )",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let blobs_deleted = u64::try_from(blobs_deleted).map_err(|_| Error::NotAProject)?;
        let blob_bytes_deleted =
            u64::try_from(blob_bytes_deleted).map_err(|_| Error::NotAProject)?;
        transaction.execute(
            "DELETE FROM blobs
             WHERE NOT EXISTS (
                 SELECT 1 FROM koharu_live_blobs live WHERE live.id = blobs.id
             )",
            [],
        )?;
        transaction.execute("DELETE FROM koharu_live_blobs", [])?;
        transaction.commit()?;
        Ok(GcReport {
            commits_deleted,
            blobs_deleted,
            blob_bytes_deleted,
        })
    }

    pub fn backup(&self, path: impl AsRef<Path>) -> Result<()> {
        self.ensure_usable()?;
        self.connection.backup(MAIN_DB, path.as_ref(), None)?;
        Ok(())
    }
}

fn wrong_kind(node: NodeId, expected: &'static str, actual: &NodeKind) -> Error {
    Error::WrongNodeKind {
        node,
        expected,
        actual: actual.name(),
    }
}

fn validate_canvas(size: CanvasSize) -> Result<()> {
    if size.width == 0 || size.height == 0 {
        Err(Error::invalid("page dimensions must be non-zero"))
    } else {
        Ok(())
    }
}

fn validate_opacity(opacity: f32) -> Result<()> {
    if opacity.is_finite() && (0.0..=1.0).contains(&opacity) {
        Ok(())
    } else {
        Err(Error::invalid("opacity must be finite and in 0..=1"))
    }
}

fn rollback(scene: &mut Scene, forward: &[StoredOp]) {
    for op in forward.iter().rev() {
        op.apply_backward(scene)
            .expect("validated scene operation must be reversible");
    }
}

fn rollback_batches(scene: &mut Scene, batches: &[StoredBatch]) {
    for batch in batches.iter().rev() {
        for op in batch.ops.iter().rev() {
            op.apply_backward(scene)
                .expect("replayed stored batch must be reversible");
        }
    }
}

fn receipt(connection: &rusqlite::Connection, command: CommandId) -> Result<Option<Receipt>> {
    let mut statement = connection.prepare_cached(
        "SELECT revision, parent_revision, command_hash, forward_batch
         FROM commits WHERE command_id = ?1",
    )?;
    let row = statement
        .query_row([command.as_uuid().as_bytes().as_slice()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })
        .optional()?;
    row.map(|(revision, parent, hash, forward)| {
        let request_hash: [u8; 32] = hash.try_into().map_err(|_| Error::NotAProject)?;
        let revision = storage::revision_from_sql(revision)?;
        let parent = storage::revision_from_sql(parent)?;
        let forward: StoredBatch = postcard::from_bytes(&forward)?;
        validate_stored_version(&forward)?;
        Ok(Receipt {
            revision,
            request_hash,
            changes: ChangeSet::from_ops(parent, revision, &forward.ops),
        })
    })
    .transpose()
}

fn receipt_result(receipt: Receipt, request_hash: [u8; 32]) -> Result<Applied> {
    if receipt.request_hash != request_hash {
        return Err(Error::CommandIdConflict);
    }
    Ok(Applied {
        revision: receipt.revision,
        changes: receipt.changes,
        already_applied: true,
    })
}

fn load_scene(connection: &rusqlite::Connection, project: &storage::ProjectRow) -> Result<Scene> {
    let mut scene = if let Some(checkpoint) = project.checkpoint {
        let bytes = connection
            .query_row(
                "SELECT checkpoint FROM commits WHERE revision = ?1",
                [storage::revision_to_sql(checkpoint)?],
                |row| row.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .flatten()
            .ok_or(Error::NotAProject)?;
        let snapshot: SceneSnapshot = postcard::from_bytes(&bytes)?;
        if snapshot.revision != checkpoint {
            return Err(Error::NotAProject);
        }
        Scene::from_snapshot(&snapshot)?
    } else {
        Scene::default()
    };

    let start = scene.revision();
    replay_records(connection, &mut scene, start, project.head)?;
    if scene.revision() != project.head {
        return Err(Error::NotAProject);
    }
    Ok(scene)
}

fn replay_records(
    connection: &rusqlite::Connection,
    scene: &mut Scene,
    after: Revision,
    through: Revision,
) -> Result<()> {
    let mut statement = connection.prepare_cached(
        "SELECT revision, parent_revision, forward_batch
         FROM commits
         WHERE revision > ?1 AND revision <= ?2
         ORDER BY revision",
    )?;
    let mut rows = statement.query(params![
        storage::revision_to_sql(after)?,
        storage::revision_to_sql(through)?
    ])?;
    while let Some(row) = rows.next()? {
        let revision = storage::revision_from_sql(row.get(0)?)?;
        let parent = storage::revision_from_sql(row.get(1)?)?;
        if scene.revision() != parent {
            return Err(Error::NotAProject);
        }
        let bytes = row.get::<_, Vec<u8>>(2)?;
        let forward: StoredBatch = postcard::from_bytes(&bytes)?;
        validate_stored_version(&forward)?;
        for op in &forward.ops {
            op.apply(scene)?;
        }
        scene.set_revision(revision);
    }
    Ok(())
}

fn load_records(
    connection: &rusqlite::Connection,
    after: Revision,
    through: Revision,
) -> Result<Vec<CommitRecord>> {
    let mut statement = connection.prepare_cached(
        "SELECT revision, parent_revision, forward_batch
         FROM commits
         WHERE revision > ?1 AND revision <= ?2
         ORDER BY revision",
    )?;
    let mut rows = statement.query(params![
        storage::revision_to_sql(after)?,
        storage::revision_to_sql(through)?
    ])?;
    let mut records = Vec::new();
    while let Some(row) = rows.next()? {
        let revision = storage::revision_from_sql(row.get(0)?)?;
        let parent = storage::revision_from_sql(row.get(1)?)?;
        let bytes = row.get::<_, Vec<u8>>(2)?;
        let forward: StoredBatch = postcard::from_bytes(&bytes)?;
        validate_stored_version(&forward)?;
        let changes = ChangeSet::from_ops(parent, revision, &forward.ops);
        records.push(CommitRecord {
            revision,
            parent,
            forward,
            changes,
        });
    }
    Ok(records)
}

fn verify_scene_blobs(connection: &rusqlite::Connection, scene: &Scene) -> Result<()> {
    let ids = scene.blob_ids().collect::<BTreeSet<_>>();
    verify_blob_ids(connection, ids)
}

fn verify_blob_ids(
    connection: &rusqlite::Connection,
    ids: impl IntoIterator<Item = BlobId>,
) -> Result<()> {
    const CHUNK_SIZE: usize = 256;

    let mut ids = ids.into_iter().collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    for chunk in ids.chunks(CHUNK_SIZE) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT COUNT(*) FROM blobs WHERE id IN ({placeholders})");
        let found = connection.query_row(
            &sql,
            rusqlite::params_from_iter(chunk.iter().map(|id| id.as_bytes().as_slice())),
            |row| row.get::<_, i64>(0),
        )?;
        if usize::try_from(found).map_err(|_| Error::NotAProject)? != chunk.len() {
            let mut statement = connection.prepare_cached("SELECT 1 FROM blobs WHERE id = ?1")?;
            for id in chunk {
                let exists = statement
                    .query_row([id.as_bytes().as_slice()], |_| Ok(()))
                    .optional()?
                    .is_some();
                if !exists {
                    return Err(Error::invalid(format!(
                        "scene references missing blob {id}"
                    )));
                }
            }
            return Err(Error::NotAProject);
        }
    }
    Ok(())
}

fn validate_stored_version(batch: &StoredBatch) -> Result<()> {
    if batch.version == 1 {
        Ok(())
    } else {
        Err(Error::NotAProject)
    }
}

fn stored_batch(connection: &rusqlite::Connection, revision: Revision) -> Result<StoredBatch> {
    let forward = connection
        .query_row(
            "SELECT forward_batch FROM commits WHERE revision = ?1",
            [storage::revision_to_sql(revision)?],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .ok_or(Error::RevisionNotRetained(revision))?;
    let forward: StoredBatch = postcard::from_bytes(&forward)?;
    validate_stored_version(&forward)?;
    Ok(forward)
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use image::{DynamicImage, GrayImage, ImageFormat, RgbaImage};
    use tempfile::tempdir;

    use super::*;
    use crate::{
        CanvasSize, CommandBatch, Page, PagePosition, Parent, Position, TextEffect, TextEffectKind,
        TextLayout, TextStyle, Transform, VerticalAlign, WalkEvent, node,
    };

    fn rgba_png(value: u8) -> Arc<[u8]> {
        let image = RgbaImage::from_pixel(2, 3, image::Rgba([value, 20, 30, 255]));
        encode(DynamicImage::ImageRgba8(image))
    }

    fn mask_png(value: u8) -> Arc<[u8]> {
        let image = GrayImage::from_pixel(2, 3, image::Luma([value]));
        encode(DynamicImage::ImageLuma8(image))
    }

    fn encode(image: DynamicImage) -> Arc<[u8]> {
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner().into()
    }

    #[test]
    fn fluent_edit_builds_mask_subtree_and_reads_blob() {
        let image_bytes = rgba_png(10);
        let mask_bytes = mask_png(200);
        let mut session = Session::memory(SessionConfig::default()).unwrap();

        let mut edit = session.edit();
        let page = edit
            .create_page(Page::new("Page", CanvasSize::new(100, 200)))
            .unwrap();
        edit.page(page).unwrap().rename("Canvas").unwrap();
        edit.page(page)
            .unwrap()
            .resize(CanvasSize::new(120, 240))
            .unwrap();
        let mask = edit
            .page(page)
            .unwrap()
            .create(node::mask(mask_bytes.clone()).named("Mask"))
            .unwrap();
        let image = edit
            .page(page)
            .unwrap()
            .container(mask)
            .unwrap()
            .create(node::image(image_bytes.clone()).named("Image"))
            .unwrap();
        let text = edit
            .page(page)
            .unwrap()
            .create(node::text(
                "hello",
                TextStyle::default(),
                TextLayout::default(),
            ))
            .unwrap();
        edit.page(page)
            .unwrap()
            .node(text)
            .unwrap()
            .set_opacity(0.5)
            .unwrap();
        let applied = edit.commit().unwrap();

        assert_eq!(applied.revision.get(), 1);
        assert_eq!(applied.changes.created_nodes().len(), 3);
        let page_ref = session.scene().page(page).unwrap();
        assert_eq!(page_ref.name(), "Canvas");
        assert_eq!(page_ref.size(), CanvasSize::new(120, 240));
        assert_eq!(
            page_ref.image(image).unwrap().natural_size(),
            PixelSize::new(2, 3)
        );
        assert_eq!(page_ref.text(text).unwrap().text(), "hello");
        let stored = session.blob(page_ref.image(image).unwrap().blob()).unwrap();
        assert_eq!(stored.as_ref(), image_bytes.as_ref());

        let events = page_ref.walk().collect::<Vec<_>>();
        assert!(matches!(events[0], WalkEvent::Enter(visit) if visit.node().id() == mask));
        assert!(matches!(events[1], WalkEvent::Enter(visit) if visit.node().id() == image));
        assert!(matches!(events[2], WalkEvent::Exit(id) if id == mask));
        assert!(matches!(events[3], WalkEvent::Enter(visit) if visit.node().id() == text));
    }

    #[test]
    fn direct_batch_is_persistent_and_backup_is_complete() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("project.koharu");
        let backup = directory.path().join("backup.koharu");
        let bytes = rgba_png(42);

        let (page, image, blob_id) = {
            let mut session = Session::create(&path, SessionConfig::default()).unwrap();
            let mut commands = CommandBatch::new(session.revision());
            let page = commands
                .create_page(Page::new("Page", CanvasSize::new(20, 30)))
                .unwrap();
            let image = commands
                .create(
                    Parent::Page(page),
                    Position::Top,
                    node::image(bytes.clone()),
                )
                .unwrap();
            session.apply(commands).unwrap();
            let blob = session
                .scene()
                .page(page)
                .unwrap()
                .image(image)
                .unwrap()
                .blob();
            session.backup(&backup).unwrap();
            (page, image, blob)
        };

        let session = Session::open(&path, SessionConfig::default()).unwrap();
        assert_eq!(
            session
                .scene()
                .page(page)
                .unwrap()
                .image(image)
                .unwrap()
                .blob(),
            blob_id
        );
        assert_eq!(session.blob(blob_id).unwrap().as_ref(), bytes.as_ref());

        let backup = Session::open(&backup, SessionConfig::default()).unwrap();
        assert_eq!(
            backup
                .scene()
                .page(page)
                .unwrap()
                .image(image)
                .unwrap()
                .blob(),
            blob_id
        );
        assert_eq!(backup.blob(blob_id).unwrap().as_ref(), bytes.as_ref());
    }

    #[test]
    fn failed_batch_rolls_back_scene_and_blobs() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut commands = CommandBatch::new(Revision::ZERO);
        let page = commands
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        let image = commands
            .create(Parent::Page(page), Position::Top, node::image(rgba_png(1)))
            .unwrap();
        commands.set_opacity(image, 2.0).unwrap();

        assert!(matches!(
            session.apply(commands),
            Err(Error::InvalidCommand(_))
        ));
        assert_eq!(session.revision(), Revision::ZERO);
        assert_eq!(session.scene().page_count(), 0);
        assert_eq!(
            session
                .connection
                .query_row("SELECT count(*) FROM blobs", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn mask_rejects_multichannel_payload() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut commands = CommandBatch::new(Revision::ZERO);
        let page = commands
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        commands
            .create(Parent::Page(page), Position::Top, node::mask(rgba_png(2)))
            .unwrap();
        assert!(matches!(
            session.apply(commands),
            Err(Error::InvalidCommand(_))
        ));
        assert_eq!(session.scene().page_count(), 0);
    }

    #[test]
    fn identical_command_retry_is_idempotent() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut commands = CommandBatch::new(Revision::ZERO);
        commands
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        let retry = commands.clone();

        let first = session.apply(commands).unwrap();
        let second = session.apply(retry).unwrap();
        assert_eq!(first.revision, second.revision);
        assert!(second.already_applied);
        assert_eq!(session.scene().page_count(), 1);
    }

    #[test]
    fn reused_command_id_with_different_request_is_rejected() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let id = CommandId::new();
        let mut first = CommandBatch::with_id(Revision::ZERO, id);
        first
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        session.apply(first).unwrap();

        let mut second = CommandBatch::with_id(Revision::ZERO, id);
        second
            .create_page(Page::new("Different", CanvasSize::new(10, 10)))
            .unwrap();
        assert!(matches!(
            session.apply(second),
            Err(Error::CommandIdConflict)
        ));
    }

    #[test]
    fn competing_session_refreshes_then_writes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("project.koharu");
        let mut first = Session::create(&path, SessionConfig::default()).unwrap();
        let mut second = Session::open(&path, SessionConfig::default()).unwrap();

        let mut one = CommandBatch::new(Revision::ZERO);
        one.create_page(Page::new("One", CanvasSize::new(10, 10)))
            .unwrap();
        first.apply(one).unwrap();

        let mut stale = CommandBatch::new(Revision::ZERO);
        stale
            .create_page(Page::new("Stale", CanvasSize::new(10, 10)))
            .unwrap();
        assert!(matches!(
            second.apply(stale),
            Err(Error::StaleSession { .. })
        ));
        let refreshed = second.refresh().unwrap();
        assert_eq!(refreshed.to_revision().get(), 1);
        assert_eq!(second.scene().page_count(), 1);

        let mut current = CommandBatch::new(second.revision());
        current
            .create_page(Page::new("Two", CanvasSize::new(10, 10)))
            .unwrap();
        second.apply(current).unwrap();
        first.refresh().unwrap();
        assert_eq!(first.scene().page_count(), 2);
    }

    #[test]
    fn refresh_reports_reload_when_incremental_history_was_pruned() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("project.koharu");
        let mut writer = Session::create(&path, SessionConfig::default()).unwrap();
        let mut stale = Session::open(&path, SessionConfig::default()).unwrap();
        let mut batch = CommandBatch::new(Revision::ZERO);
        batch
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        let head = writer.apply(batch).unwrap().revision;
        writer.prune_history(head).unwrap();

        let changes = stale.refresh().unwrap();
        assert!(changes.requires_reload());
        assert_eq!(stale.revision(), head);
        assert_eq!(stale.scene().page_count(), 1);
    }

    #[test]
    fn arbitrary_revert_preserves_unrelated_later_change() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut create = CommandBatch::new(Revision::ZERO);
        let page = create
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        let text = create
            .create(
                Parent::Page(page),
                Position::Top,
                node::text("hello", TextStyle::default(), TextLayout::default()),
            )
            .unwrap();
        session.apply(create).unwrap();

        let mut opacity = CommandBatch::new(session.revision());
        opacity.set_opacity(text, 0.25).unwrap();
        let opacity_revision = session.apply(opacity).unwrap().revision;

        let mut name = CommandBatch::new(session.revision());
        name.set_name(text, Some("Caption".into())).unwrap();
        session.apply(name).unwrap();

        session.revert(&[opacity_revision]).unwrap();
        let node = session.scene().node(text).unwrap();
        assert_eq!(node.opacity(), 1.0);
        assert_eq!(node.name(), Some("Caption"));
    }

    #[test]
    fn revert_rejects_changed_precondition() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut create = CommandBatch::new(Revision::ZERO);
        let page = create
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        let node = create
            .create(Parent::Page(page), Position::Top, node::group())
            .unwrap();
        session.apply(create).unwrap();

        let mut first = CommandBatch::new(session.revision());
        first.set_opacity(node, 0.5).unwrap();
        let revision = session.apply(first).unwrap().revision;
        let mut second = CommandBatch::new(session.revision());
        second.set_opacity(node, 0.25).unwrap();
        session.apply(second).unwrap();

        assert!(matches!(
            session.revert(&[revision]),
            Err(Error::HistoryConflict(_))
        ));
        assert_eq!(session.scene().node(node).unwrap().opacity(), 0.25);
    }

    #[test]
    fn cross_page_move_preserves_subtree() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut create = CommandBatch::new(Revision::ZERO);
        let first = create
            .create_page(Page::new("One", CanvasSize::new(10, 10)))
            .unwrap();
        let second = create
            .create_page(Page::new("Two", CanvasSize::new(10, 10)))
            .unwrap();
        let group = create
            .create(Parent::Page(first), Position::Top, node::group())
            .unwrap();
        let text = create
            .create(
                Parent::Node(group),
                Position::Top,
                node::text("child", TextStyle::default(), TextLayout::default()),
            )
            .unwrap();
        session.apply(create).unwrap();

        let mut movement = CommandBatch::new(session.revision());
        movement
            .move_node(group, Parent::Page(second), Position::Top)
            .unwrap();
        session.apply(movement).unwrap();

        assert!(session.scene().page(first).unwrap().node(group).is_err());
        assert!(session.scene().page(second).unwrap().node(group).is_ok());
        assert!(session.scene().page(second).unwrap().node(text).is_ok());
    }

    #[test]
    fn same_page_reorder_uses_semantic_positions() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut create = CommandBatch::new(Revision::ZERO);
        let page = create
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        let first = create
            .create(Parent::Page(page), Position::Top, node::group())
            .unwrap();
        let second = create
            .create(Parent::Page(page), Position::Top, node::group())
            .unwrap();
        let third = create
            .create(Parent::Page(page), Position::Top, node::group())
            .unwrap();
        session.apply(create).unwrap();
        assert_eq!(
            session
                .scene()
                .page(page)
                .unwrap()
                .children()
                .unwrap()
                .collect::<Vec<_>>(),
            vec![first, second, third]
        );

        let mut no_op = CommandBatch::new(session.revision());
        no_op.place_above(second, first).unwrap();
        session.apply(no_op).unwrap();

        let mut reorder = CommandBatch::new(session.revision());
        reorder.place_above(first, third).unwrap();
        session.apply(reorder).unwrap();
        assert_eq!(
            session
                .scene()
                .page(page)
                .unwrap()
                .children()
                .unwrap()
                .collect::<Vec<_>>(),
            vec![second, third, first]
        );
    }

    #[test]
    fn pages_can_be_reordered_without_rebuilding_them() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut create = CommandBatch::new(Revision::ZERO);
        let first = create
            .create_page(Page::new("One", CanvasSize::new(10, 10)))
            .unwrap();
        let second = create
            .create_page(Page::new("Two", CanvasSize::new(10, 10)))
            .unwrap();
        let third = create
            .create_page(Page::new("Three", CanvasSize::new(10, 10)))
            .unwrap();
        session.apply(create).unwrap();

        let mut reorder = CommandBatch::new(session.revision());
        reorder
            .move_page(first, PagePosition::After(third))
            .unwrap();
        session.apply(reorder).unwrap();
        assert_eq!(
            session.scene().page_ids().collect::<Vec<_>>(),
            vec![second, third, first]
        );
    }

    #[test]
    fn pruning_collects_blobs_outside_current_scene_and_retained_history() {
        let first_bytes = rgba_png(1);
        let second_bytes = rgba_png(2);
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut create = CommandBatch::new(Revision::ZERO);
        let page = create
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        let image = create
            .create(Parent::Page(page), Position::Top, node::image(first_bytes))
            .unwrap();
        session.apply(create).unwrap();
        let first_blob = session
            .scene()
            .page(page)
            .unwrap()
            .image(image)
            .unwrap()
            .blob();

        let mut replace = CommandBatch::new(session.revision());
        replace.set_image(image, second_bytes).unwrap();
        session.apply(replace).unwrap();
        let second_blob = session
            .scene()
            .page(page)
            .unwrap()
            .image(image)
            .unwrap()
            .blob();

        let mut metadata = CommandBatch::new(session.revision());
        metadata.set_name(image, Some("Current".into())).unwrap();
        let head = session.apply(metadata).unwrap().revision;

        let report = session.prune_history(head).unwrap();
        assert_eq!(report.commits_deleted, 2);
        assert_eq!(report.blobs_deleted, 1);
        assert!(session.blob(first_blob).is_err());
        assert!(session.blob(second_blob).is_ok());
    }

    #[test]
    fn pruned_project_reopens_from_head_checkpoint() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("project.koharu");
        let (page, text, head) = {
            let mut session = Session::create(&path, SessionConfig::default()).unwrap();
            let mut create = CommandBatch::new(Revision::ZERO);
            let page = create
                .create_page(Page::new("Page", CanvasSize::new(10, 10)))
                .unwrap();
            let text = create
                .create(
                    Parent::Page(page),
                    Position::Top,
                    node::text("hello", TextStyle::default(), TextLayout::default()),
                )
                .unwrap();
            session.apply(create).unwrap();
            let mut update = CommandBatch::new(session.revision());
            update.set_opacity(text, 0.4).unwrap();
            let head = session.apply(update).unwrap().revision;
            session.prune_history(head).unwrap();
            (page, text, head)
        };

        let session = Session::open(&path, SessionConfig::default()).unwrap();
        assert_eq!(session.revision(), head);
        assert_eq!(
            session
                .scene()
                .page(page)
                .unwrap()
                .node(text)
                .unwrap()
                .opacity(),
            0.4
        );
    }

    #[test]
    fn commit_schema_stores_one_canonical_batch() {
        let session = Session::memory(SessionConfig::default()).unwrap();
        let mut statement = session
            .connection
            .prepare("PRAGMA table_info(commits)")
            .unwrap();
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            columns,
            [
                "revision",
                "parent_revision",
                "command_id",
                "command_hash",
                "forward_batch",
                "blob_refs",
                "checkpoint",
            ]
        );
    }

    #[test]
    fn automatic_checkpoint_replaces_the_previous_snapshot() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("automatic-checkpoint.koharu");
        let config = SessionConfig {
            checkpoint_interval: Some(2),
            ..SessionConfig::default()
        };
        let mut session = Session::create(&path, config.clone()).unwrap();
        let mut create = CommandBatch::new(Revision::ZERO);
        let page = create
            .create_page(Page::new("0", CanvasSize::new(100, 100)))
            .unwrap();
        session.apply(create).unwrap();

        for name in ["1", "2", "3"] {
            let mut update = CommandBatch::new(session.revision());
            update.rename_page(page, name).unwrap();
            session.apply(update).unwrap();
        }
        assert_eq!(session.revision(), Revision::new(4));
        let project = storage::project(&session.connection).unwrap();
        assert_eq!(project.checkpoint, Some(Revision::new(4)));
        let checkpoint_count = session
            .connection
            .query_row(
                "SELECT COUNT(*) FROM commits WHERE checkpoint IS NOT NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(checkpoint_count, 1);

        drop(session);
        let reopened = Session::open(&path, config).unwrap();
        assert_eq!(reopened.scene().page(page).unwrap().name(), "3");
    }

    #[test]
    fn text_history_stores_fields_and_derives_undo() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut create = CommandBatch::new(Revision::ZERO);
        let page = create
            .create_page(Page::new("Page", CanvasSize::new(100, 100)))
            .unwrap();
        let text = create
            .create(
                Parent::Page(page),
                Position::Top,
                node::text("old", TextStyle::default(), TextLayout::default()),
            )
            .unwrap();
        session.apply(create).unwrap();

        let new_style = TextStyle {
            font_size: 24.0,
            ..TextStyle::default()
        };
        let new_layout = TextLayout {
            max_width: Some(80.0),
            ..TextLayout::default()
        };
        let mut update = CommandBatch::new(session.revision());
        update.set_text(text, "new").unwrap();
        update.set_text_style(text, new_style.clone()).unwrap();
        update.set_text_layout(text, new_layout.clone()).unwrap();
        let revision = session.apply(update).unwrap().revision;

        let stored = stored_batch(&session.connection, revision).unwrap();
        assert!(matches!(stored.ops[0], StoredOp::SetText { .. }));
        assert!(matches!(stored.ops[1], StoredOp::SetTextStyle { .. }));
        assert!(matches!(stored.ops[2], StoredOp::SetTextLayout { .. }));

        session.revert(&[revision]).unwrap();
        let restored = session.scene().page(page).unwrap().text(text).unwrap();
        assert_eq!(restored.text(), "old");
        assert_eq!(restored.style(), &TextStyle::default());
        assert_eq!(restored.layout(), &TextLayout::default());
    }

    #[test]
    fn photoshop_text_style_survives_storage() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("text-style.koharu");
        let style = TextStyle {
            font_families: vec!["Aptos".into(), "Noto Sans".into()],
            angle_degrees: -8.0,
            vertical_align: VerticalAlign::Bottom,
            effects: vec![TextEffect::new(TextEffectKind::Stroke {
                color: [255, 255, 255, 255],
                width: 3.0,
                position: crate::StrokePosition::Outside,
            })],
            ..TextStyle::default()
        };
        let (page, text) = {
            let mut session = Session::create(&path, SessionConfig::default()).unwrap();
            let mut batch = CommandBatch::new(Revision::ZERO);
            let page = batch
                .create_page(Page::new("Page", CanvasSize::new(100, 100)))
                .unwrap();
            let text = batch
                .create(
                    Parent::Page(page),
                    Position::Top,
                    node::text("Styled", style.clone(), TextLayout::default()),
                )
                .unwrap();
            session.apply(batch).unwrap();
            session.checkpoint().unwrap();
            (page, text)
        };

        let session = Session::open(&path, SessionConfig::default()).unwrap();
        assert_eq!(
            session
                .scene()
                .page(page)
                .unwrap()
                .text(text)
                .unwrap()
                .style(),
            &style
        );
    }

    #[test]
    fn transforms_accumulate_during_walk() {
        let mut session = Session::memory(SessionConfig::default()).unwrap();
        let mut batch = CommandBatch::new(Revision::ZERO);
        let page = batch
            .create_page(Page::new("Page", CanvasSize::new(10, 10)))
            .unwrap();
        let group = batch
            .create(
                Parent::Page(page),
                Position::Top,
                node::group().at(Transform::translation(10.0, 20.0)),
            )
            .unwrap();
        let text = batch
            .create(
                Parent::Node(group),
                Position::Top,
                node::text("x", TextStyle::default(), TextLayout::default())
                    .at(Transform::translation(2.0, 3.0)),
            )
            .unwrap();
        session.apply(batch).unwrap();
        let visit = session
            .scene()
            .page(page)
            .unwrap()
            .walk()
            .find_map(|event| match event {
                WalkEvent::Enter(visit) if visit.node().id() == text => Some(visit),
                _ => None,
            })
            .unwrap();
        assert_eq!(visit.world_transform().tx, 12.0);
        assert_eq!(visit.world_transform().ty, 23.0);
    }
}
