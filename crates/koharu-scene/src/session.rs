use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::Path,
    sync::Arc,
    time::Duration,
};

use revision::revisioned;
use rusqlite::{MAIN_DB, OptionalExtension, TransactionBehavior, params};

use crate::{
    BlobId, Command, Commands, Element, ElementChange, ElementId, ElementKind, Error, Page,
    PageAsset, PageId, Project, ProjectId, Result, Revision, Size, blob,
    command::{PositionedElement, PositionedPage, StoredBatch, StoredChange},
    storage,
};

#[derive(Clone, Debug)]
pub struct Options {
    pub busy_timeout: Duration,
    pub checkpoint_interval: Option<u64>,
    pub max_blob_bytes: usize,
    pub max_image_pixels: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            busy_timeout: Duration::from_secs(5),
            checkpoint_interval: Some(1_024),
            max_blob_bytes: 512 * 1024 * 1024,
            max_image_pixels: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangeSet {
    pub from: Revision,
    pub to: Revision,
    pub pages: Vec<PageId>,
    pub elements: Vec<ElementId>,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct GcReport {
    pub blobs: usize,
    pub bytes: u64,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    revision: Revision,
    project: Project,
}

struct State {
    project: Project,
    pages: HashMap<PageId, usize>,
    elements: HashMap<ElementId, (usize, usize)>,
}

pub struct Session {
    connection: rusqlite::Connection,
    id: ProjectId,
    revision: Revision,
    state: State,
    options: Options,
}

impl Session {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::create_with(path, Options::default())
    }

    pub fn create_with(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        if path.as_ref().exists() {
            return Err(Error::invalid("project file already exists"));
        }
        let connection = storage::create_disk(path.as_ref(), options.busy_timeout)?;
        Self::initialize(connection, options)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, Options::default())
    }

    pub fn open_with(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        let connection = storage::open_disk(path.as_ref(), options.busy_timeout)?;
        Self::load(connection, options)
    }

    pub fn memory() -> Result<Self> {
        Self::memory_with(Options::default())
    }

    pub fn memory_with(options: Options) -> Result<Self> {
        let connection = storage::open_memory(options.busy_timeout)?;
        Self::initialize(connection, options)
    }

    fn initialize(connection: rusqlite::Connection, options: Options) -> Result<Self> {
        let id = ProjectId::new();
        let project = Project::new();
        let snapshot = revision::to_vec(&Snapshot {
            revision: Revision::ZERO,
            project: project.clone(),
        })?;
        storage::create_schema(&connection, id, &snapshot)?;
        Ok(Self {
            connection,
            id,
            revision: Revision::ZERO,
            state: State::new(project)?,
            options,
        })
    }

    fn load(connection: rusqlite::Connection, options: Options) -> Result<Self> {
        let row = storage::project(&connection)?;
        let snapshot: Snapshot = revision::from_slice(&row.checkpoint)?;
        if snapshot.revision != row.checkpoint_revision || snapshot.revision > row.head {
            return Err(Error::NotAProject);
        }
        let mut state = State::new(snapshot.project)?;
        replay(&connection, &mut state, snapshot.revision, row.head)?;
        verify_blob_ids(&connection, current_blob_ids(&state.project))?;
        Ok(Self {
            connection,
            id: row.id,
            revision: row.head,
            state,
            options,
        })
    }

    #[must_use]
    pub const fn id(&self) -> ProjectId {
        self.id
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn project(&self) -> &Project {
        &self.state.project
    }

    pub fn page(&self, id: PageId) -> Result<&Page> {
        self.state.page(id)
    }

    pub fn element(&self, id: ElementId) -> Result<(&Page, &Element)> {
        self.state.element(id)
    }

    #[must_use]
    pub fn commands(&self) -> Commands {
        Commands::new(self.revision)
    }

    pub fn read_blob(&self, id: BlobId) -> Result<Arc<[u8]>> {
        self.connection
            .query_row(
                "SELECT bytes FROM blobs WHERE id = ?1",
                [id.as_bytes()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?
            .map(Arc::from)
            .ok_or_else(|| Error::invalid(format!("blob {id} was not found")))
    }

    pub fn apply(&mut self, commands: Commands) -> Result<ChangeSet> {
        if commands.base != self.revision {
            return Err(Error::RevisionConflict {
                expected: commands.base,
                actual: self.revision,
            });
        }
        self.validate_command_blobs(&commands)?;
        let batch = self.state.prepare(commands.ops)?;
        if batch.changes.is_empty() {
            return Ok(ChangeSet {
                from: self.revision,
                to: self.revision,
                ..ChangeSet::default()
            });
        }
        self.persist_applied(batch, &commands.attachments)
    }

    pub fn refresh(&mut self) -> Result<ChangeSet> {
        let from = self.revision;
        let transaction = self.connection.transaction()?;
        let head = storage::head(&transaction)?;
        if head == from {
            transaction.commit()?;
            return Ok(ChangeSet {
                from,
                to: from,
                ..ChangeSet::default()
            });
        }
        if head < from {
            return Err(Error::NotAProject);
        }

        let first_parent = transaction
            .query_row(
                "SELECT parent_revision FROM commits WHERE revision > ?1 ORDER BY revision LIMIT 1",
                [storage::revision_to_sql(from)?],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(storage::revision_from_sql)
            .transpose()?;

        let mut changes = ChangeSet {
            from,
            to: head,
            ..ChangeSet::default()
        };
        if first_parent == Some(from) {
            replay_collect(&transaction, &mut self.state, from, head, &mut changes)?;
        } else {
            let row = storage::project(&transaction)?;
            let snapshot: Snapshot = revision::from_slice(&row.checkpoint)?;
            if snapshot.revision != row.checkpoint_revision {
                return Err(Error::NotAProject);
            }
            self.state = State::new(snapshot.project)?;
            replay_collect(
                &transaction,
                &mut self.state,
                snapshot.revision,
                head,
                &mut changes,
            )?;
            changes.pages = self
                .state
                .project
                .pages
                .iter()
                .map(|page| page.id)
                .collect();
            changes.elements = self
                .state
                .project
                .pages
                .iter()
                .flat_map(|page| page.elements.iter().map(|element| element.id))
                .collect();
        }
        transaction.commit()?;
        self.revision = head;
        Ok(changes)
    }

    pub fn revert(&mut self, revisions: impl IntoIterator<Item = Revision>) -> Result<ChangeSet> {
        let mut revisions = revisions.into_iter().collect::<Vec<_>>();
        revisions.sort_unstable_by(|left, right| right.cmp(left));
        revisions.dedup();
        let mut changes = Vec::new();
        for revision in revisions {
            let bytes = self
                .connection
                .query_row(
                    "SELECT changes FROM commits WHERE revision = ?1",
                    [storage::revision_to_sql(revision)?],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()?
                .ok_or(Error::HistoryNotFound(revision))?;
            changes.extend(
                revision::from_slice::<StoredBatch>(&bytes)?
                    .reversed()
                    .changes,
            );
        }
        let batch = StoredBatch { changes };
        self.state.apply_batch(&batch)?;
        if batch.changes.is_empty() {
            return Ok(ChangeSet {
                from: self.revision,
                to: self.revision,
                ..ChangeSet::default()
            });
        }
        self.persist_applied(batch, &HashMap::new())
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        let bytes = revision::to_vec(&Snapshot {
            revision: self.revision,
            project: self.state.project.clone(),
        })?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual = storage::head(&transaction)?;
        if actual != self.revision {
            return Err(Error::RevisionConflict {
                expected: self.revision,
                actual,
            });
        }
        transaction.execute(
            "UPDATE project SET checkpoint_revision = ?1, checkpoint = ?2 WHERE singleton = 1",
            params![storage::revision_to_sql(self.revision)?, bytes],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn prune_history(&mut self, keep_from: Revision) -> Result<GcReport> {
        if keep_from > self.revision.next().unwrap_or(self.revision) {
            return Err(Error::invalid("history retention begins after the head"));
        }
        let snapshot = revision::to_vec(&Snapshot {
            revision: self.revision,
            project: self.state.project.clone(),
        })?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual = storage::head(&transaction)?;
        if actual != self.revision {
            return Err(Error::RevisionConflict {
                expected: self.revision,
                actual,
            });
        }
        transaction.execute(
            "UPDATE project SET checkpoint_revision = ?1, checkpoint = ?2 WHERE singleton = 1",
            params![storage::revision_to_sql(self.revision)?, snapshot],
        )?;
        transaction.execute(
            "DELETE FROM commits WHERE revision < ?1",
            [storage::revision_to_sql(keep_from)?],
        )?;
        let report = collect_garbage(&transaction, &self.state.project)?;
        transaction.commit()?;
        Ok(report)
    }

    pub fn gc(&mut self) -> Result<GcReport> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual = storage::head(&transaction)?;
        if actual != self.revision {
            return Err(Error::RevisionConflict {
                expected: self.revision,
                actual,
            });
        }
        let report = collect_garbage(&transaction, &self.state.project)?;
        transaction.commit()?;
        Ok(report)
    }

    pub fn backup(&self, path: impl AsRef<Path>) -> Result<()> {
        self.connection.backup(MAIN_DB, path.as_ref(), None)?;
        Ok(())
    }

    fn persist_applied(
        &mut self,
        batch: StoredBatch,
        attachments: &HashMap<BlobId, blob::Attachment>,
    ) -> Result<ChangeSet> {
        let parent = self.revision;
        let next = parent
            .next()
            .ok_or_else(|| Error::invalid("revision overflow"))?;
        let changes_bytes = revision::to_vec(&batch)?;
        let checkpoint = self
            .options
            .checkpoint_interval
            .filter(|interval| *interval != 0 && next.get() % interval == 0)
            .map(|_| {
                revision::to_vec(&Snapshot {
                    revision: next,
                    project: self.state.project.clone(),
                })
            })
            .transpose()?;

        let result = (|| {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let actual = storage::head(&transaction)?;
            if actual != parent {
                return Err(Error::RevisionConflict {
                    expected: parent,
                    actual,
                });
            }

            let mut referenced = HashSet::new();
            batch.blob_ids(&mut referenced);
            for id in &referenced {
                if let Some(attachment) = attachments.get(id) {
                    transaction.execute(
                        "INSERT OR IGNORE INTO blobs (id, bytes) VALUES (?1, ?2)",
                        params![id.as_bytes(), attachment.bytes.as_ref()],
                    )?;
                }
            }
            verify_blob_ids(&transaction, referenced)?;
            transaction.execute(
                "INSERT INTO commits (revision, parent_revision, changes) VALUES (?1, ?2, ?3)",
                params![
                    storage::revision_to_sql(next)?,
                    storage::revision_to_sql(parent)?,
                    changes_bytes,
                ],
            )?;
            if let Some(checkpoint) = checkpoint {
                transaction.execute(
                    "UPDATE project SET head_revision = ?1, checkpoint_revision = ?1,
                     checkpoint = ?2 WHERE singleton = 1",
                    params![storage::revision_to_sql(next)?, checkpoint],
                )?;
            } else {
                transaction.execute(
                    "UPDATE project SET head_revision = ?1 WHERE singleton = 1",
                    [storage::revision_to_sql(next)?],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })();

        if let Err(error) = result {
            self.state.apply_batch(&batch.reversed())?;
            return Err(error);
        }
        self.revision = next;
        Ok(ChangeSet::for_batch(parent, next, &batch))
    }

    fn validate_command_blobs(&self, commands: &Commands) -> Result<()> {
        let mut inspected = HashMap::<BlobId, (Size, bool)>::new();
        let mut page_sizes = self
            .state
            .project
            .pages
            .iter()
            .map(|page| (page.id, page.size))
            .collect::<HashMap<_, _>>();
        for command in &commands.ops {
            match command {
                Command::InsertPage { page, .. } => {
                    self.validate_blob(page.source, page.size, false, commands, &mut inspected)?;
                    for asset in [
                        PageAsset::Clean,
                        PageAsset::Rendered,
                        PageAsset::TextMask,
                        PageAsset::BubbleMask,
                        PageAsset::BrushMask,
                    ] {
                        if let Some(id) = page.assets.get(asset) {
                            self.validate_blob(
                                id,
                                page.size,
                                asset.is_mask(),
                                commands,
                                &mut inspected,
                            )?;
                        }
                    }
                    for element in &page.elements {
                        if let ElementKind::Image(image) = &element.kind {
                            self.validate_blob(
                                image.blob,
                                image.natural_size,
                                false,
                                commands,
                                &mut inspected,
                            )?;
                        }
                    }
                    page_sizes.insert(page.id, page.size);
                }
                Command::DeletePage(page) => {
                    page_sizes.remove(page);
                }
                Command::ReplaceSource { page, blob, size } => {
                    if page_sizes.get(page).copied() != Some(*size) {
                        return Err(Error::invalid(
                            "replacement source must preserve page dimensions",
                        ));
                    }
                    self.validate_blob(*blob, *size, false, commands, &mut inspected)?;
                }
                Command::SetPageAsset {
                    page,
                    asset,
                    blob: Some(blob),
                } => {
                    let size = page_sizes
                        .get(page)
                        .copied()
                        .ok_or(Error::PageNotFound(*page))?;
                    self.validate_blob(*blob, size, asset.is_mask(), commands, &mut inspected)?;
                }
                Command::InsertElement { element, .. } => {
                    if let ElementKind::Image(image) = &element.kind {
                        self.validate_blob(
                            image.blob,
                            image.natural_size,
                            false,
                            commands,
                            &mut inspected,
                        )?;
                    }
                }
                Command::EditElement {
                    edit: ElementChange::Image { blob, natural_size },
                    ..
                } => {
                    self.validate_blob(*blob, *natural_size, false, commands, &mut inspected)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_blob(
        &self,
        id: BlobId,
        expected: Size,
        mask: bool,
        commands: &Commands,
        inspected: &mut HashMap<BlobId, (Size, bool)>,
    ) -> Result<()> {
        let (size, single_channel) = if let Some(value) = inspected.get(&id) {
            *value
        } else if let Some(attachment) = commands.attachments.get(&id) {
            if attachment.bytes.len() > self.options.max_blob_bytes {
                return Err(Error::invalid("image attachment exceeds the byte limit"));
            }
            (attachment.size, attachment.single_channel)
        } else {
            let bytes = self.read_blob(id)?;
            if bytes.len() > self.options.max_blob_bytes {
                return Err(Error::invalid("stored image exceeds the byte limit"));
            }
            let attachment = blob::attach(bytes, false)?;
            if attachment.id != id {
                return Err(Error::invalid(format!("blob {id} has an invalid digest")));
            }
            (attachment.size, attachment.single_channel)
        };
        inspected.insert(id, (size, single_channel));
        if size != expected {
            return Err(Error::invalid(format!(
                "image dimensions {}x{} do not match {}x{}",
                size.width, size.height, expected.width, expected.height
            )));
        }
        if size.pixels() > self.options.max_image_pixels {
            return Err(Error::invalid("image exceeds the pixel limit"));
        }
        if mask && !single_channel {
            return Err(Error::invalid("mask image must have exactly one channel"));
        }
        Ok(())
    }
}

impl State {
    fn new(project: Project) -> Result<Self> {
        project.validate()?;
        let mut state = Self {
            project,
            pages: HashMap::new(),
            elements: HashMap::new(),
        };
        state.reindex()?;
        Ok(state)
    }

    fn reindex(&mut self) -> Result<()> {
        self.pages.clear();
        self.elements.clear();
        for (page_index, page) in self.project.pages.iter().enumerate() {
            if self.pages.insert(page.id, page_index).is_some() {
                return Err(Error::invalid(format!("duplicate page {}", page.id)));
            }
            for (element_index, element) in page.elements.iter().enumerate() {
                if self
                    .elements
                    .insert(element.id, (page_index, element_index))
                    .is_some()
                {
                    return Err(Error::invalid(format!("duplicate element {}", element.id)));
                }
            }
        }
        Ok(())
    }

    fn reindex_page(&mut self, page_index: usize) -> Result<()> {
        let page = &self.project.pages[page_index];
        for (element_index, element) in page.elements.iter().enumerate() {
            self.elements
                .insert(element.id, (page_index, element_index));
        }
        Ok(())
    }

    fn page(&self, id: PageId) -> Result<&Page> {
        self.pages
            .get(&id)
            .map(|index| &self.project.pages[*index])
            .ok_or(Error::PageNotFound(id))
    }

    fn page_mut(&mut self, id: PageId) -> Result<&mut Page> {
        let index = *self.pages.get(&id).ok_or(Error::PageNotFound(id))?;
        Ok(&mut self.project.pages[index])
    }

    fn element(&self, id: ElementId) -> Result<(&Page, &Element)> {
        let (page, element) = *self.elements.get(&id).ok_or(Error::ElementNotFound(id))?;
        Ok((
            &self.project.pages[page],
            &self.project.pages[page].elements[element],
        ))
    }

    fn prepare(&mut self, commands: Vec<Command>) -> Result<StoredBatch> {
        let mut changes = Vec::with_capacity(commands.len());
        for command in commands {
            match self.prepare_one(command) {
                Ok(Some(change)) => changes.push(change),
                Ok(None) => {}
                Err(error) => {
                    let applied = StoredBatch { changes };
                    self.apply_batch(&applied.reversed())?;
                    return Err(error);
                }
            }
        }
        if let Err(error) = self.project.validate() {
            let applied = StoredBatch { changes };
            self.apply_batch(&applied.reversed())?;
            return Err(error);
        }
        Ok(StoredBatch { changes })
    }

    fn prepare_one(&mut self, command: Command) -> Result<Option<StoredChange>> {
        match command {
            Command::InsertPage { page, index } => {
                if self.pages.contains_key(&page.id)
                    || page
                        .elements
                        .iter()
                        .any(|element| self.elements.contains_key(&element.id))
                {
                    return Err(Error::invalid("inserted page contains an existing ID"));
                }
                let mut ids = HashSet::new();
                page.validate(&mut ids)?;
                let index = append_index(index, self.project.pages.len())?;
                self.project.pages.insert(index, page.clone());
                self.reindex()?;
                Ok(Some(StoredChange::Page {
                    before: None,
                    after: Some(PositionedPage { index, page }),
                }))
            }
            Command::DeletePage(id) => {
                let index = *self.pages.get(&id).ok_or(Error::PageNotFound(id))?;
                let page = self.project.pages.remove(index);
                self.reindex()?;
                Ok(Some(StoredChange::Page {
                    before: Some(PositionedPage { index, page }),
                    after: None,
                }))
            }
            Command::MovePage { page, index } => {
                let before = *self.pages.get(&page).ok_or(Error::PageNotFound(page))?;
                let after = move_index(index, self.project.pages.len())?;
                if before == after {
                    return Ok(None);
                }
                let value = self.project.pages.remove(before);
                self.project.pages.insert(after, value);
                self.reindex()?;
                Ok(Some(StoredChange::MovePage {
                    page,
                    before,
                    after,
                }))
            }
            Command::RenamePage { page, name: after } => {
                let target = self.page_mut(page)?;
                let before = std::mem::replace(&mut target.name, after.clone());
                Ok((before != after).then_some(StoredChange::PageName {
                    page,
                    before,
                    after,
                }))
            }
            Command::ReplaceSource {
                page,
                blob: after_blob,
                size: _,
            } => {
                let target = self.page_mut(page)?;
                let before = target.source;
                if before == after_blob {
                    return Ok(None);
                }
                target.source = after_blob;
                Ok(Some(StoredChange::PageSource {
                    page,
                    before,
                    after: after_blob,
                }))
            }
            Command::SetPageAsset {
                page,
                asset,
                blob: after,
            } => {
                let target = self.page_mut(page)?;
                let before = target.assets.get(asset);
                if before == after {
                    return Ok(None);
                }
                target.assets.set(asset, after);
                Ok(Some(StoredChange::PageAsset {
                    page,
                    asset,
                    before,
                    after,
                }))
            }
            Command::InsertElement {
                page,
                element,
                index,
            } => {
                if self.elements.contains_key(&element.id) {
                    return Err(Error::invalid(format!(
                        "element {} already exists",
                        element.id
                    )));
                }
                element.validate()?;
                let page_index = *self.pages.get(&page).ok_or(Error::PageNotFound(page))?;
                let index = append_index(index, self.project.pages[page_index].elements.len())?;
                self.project.pages[page_index]
                    .elements
                    .insert(index, element.clone());
                self.reindex_page(page_index)?;
                Ok(Some(StoredChange::Element {
                    page,
                    before: None,
                    after: Some(PositionedElement { index, element }),
                }))
            }
            Command::DeleteElement { page, element } => {
                let (page_index, index) = *self
                    .elements
                    .get(&element)
                    .ok_or(Error::ElementNotFound(element))?;
                if self.project.pages[page_index].id != page {
                    return Err(Error::ElementNotFound(element));
                }
                let element = self.project.pages[page_index].elements.remove(index);
                self.elements.remove(&element.id);
                self.reindex_page(page_index)?;
                Ok(Some(StoredChange::Element {
                    page,
                    before: Some(PositionedElement { index, element }),
                    after: None,
                }))
            }
            Command::MoveElement {
                page,
                element,
                index,
            } => {
                let (page_index, before) = *self
                    .elements
                    .get(&element)
                    .ok_or(Error::ElementNotFound(element))?;
                if self.project.pages[page_index].id != page {
                    return Err(Error::ElementNotFound(element));
                }
                let after = move_index(index, self.project.pages[page_index].elements.len())?;
                if before == after {
                    return Ok(None);
                }
                let value = self.project.pages[page_index].elements.remove(before);
                let before_value = value.clone();
                self.project.pages[page_index]
                    .elements
                    .insert(after, value.clone());
                self.reindex_page(page_index)?;
                Ok(Some(StoredChange::Element {
                    page,
                    before: Some(PositionedElement {
                        index: before,
                        element: before_value,
                    }),
                    after: Some(PositionedElement {
                        index: after,
                        element: value,
                    }),
                }))
            }
            Command::EditElement {
                page,
                element,
                edit,
            } => {
                let (page_index, index) = *self
                    .elements
                    .get(&element)
                    .ok_or(Error::ElementNotFound(element))?;
                if self.project.pages[page_index].id != page {
                    return Err(Error::ElementNotFound(element));
                }
                let before = self.project.pages[page_index].elements[index].clone();
                let mut after = before.clone();
                apply_element_edit(&mut after, edit)?;
                after.validate()?;
                if before == after {
                    return Ok(None);
                }
                self.project.pages[page_index].elements[index] = after.clone();
                Ok(Some(StoredChange::Element {
                    page,
                    before: Some(PositionedElement {
                        index,
                        element: before,
                    }),
                    after: Some(PositionedElement {
                        index,
                        element: after,
                    }),
                }))
            }
        }
    }

    fn apply_batch(&mut self, batch: &StoredBatch) -> Result<()> {
        let mut applied: Vec<StoredChange> = Vec::new();
        for change in &batch.changes {
            if let Err(error) = self.apply_change(change) {
                for change in applied.iter().rev() {
                    self.apply_change(&change.reversed())?;
                }
                return Err(error);
            }
            applied.push(change.clone());
        }
        Ok(())
    }

    fn apply_change(&mut self, change: &StoredChange) -> Result<()> {
        match change {
            StoredChange::Page { before, after } => {
                if let Some(before) = before {
                    if self.project.pages.get(before.index) != Some(&before.page) {
                        return history("page");
                    }
                    self.project.pages.remove(before.index);
                }
                if let Some(after) = after {
                    if after.index > self.project.pages.len() {
                        return history("page index");
                    }
                    self.project.pages.insert(after.index, after.page.clone());
                }
                self.reindex()?;
            }
            StoredChange::MovePage {
                page,
                before,
                after,
            } => {
                if self.project.pages.get(*before).map(|page| page.id) != Some(*page)
                    || *after >= self.project.pages.len()
                {
                    return history("page order");
                }
                let page = self.project.pages.remove(*before);
                self.project.pages.insert(*after, page);
                self.reindex()?;
            }
            StoredChange::PageName {
                page,
                before,
                after,
            } => {
                let target = self.page_mut(*page)?;
                expect(&target.name, before, "page name")?;
                target.name.clone_from(after);
            }
            StoredChange::PageSource {
                page,
                before,
                after,
            } => {
                let target = self.page_mut(*page)?;
                expect(&target.source, before, "page source")?;
                target.source = *after;
            }
            StoredChange::PageAsset {
                page,
                asset,
                before,
                after,
            } => {
                let target = self.page_mut(*page)?;
                expect(&target.assets.get(*asset), before, "page asset")?;
                target.assets.set(*asset, *after);
            }
            StoredChange::Element {
                page,
                before,
                after,
            } => {
                let page_index = *self.pages.get(page).ok_or(Error::PageNotFound(*page))?;
                if let (Some(before), Some(after)) = (before, after)
                    && before.index == after.index
                    && before.element.id == after.element.id
                {
                    if self.project.pages[page_index].elements.get(before.index)
                        != Some(&before.element)
                    {
                        return history("element");
                    }
                    self.project.pages[page_index].elements[after.index] = after.element.clone();
                    return Ok(());
                }
                if let Some(before) = before {
                    if self.project.pages[page_index].elements.get(before.index)
                        != Some(&before.element)
                    {
                        return history("element");
                    }
                    self.project.pages[page_index].elements.remove(before.index);
                    self.elements.remove(&before.element.id);
                }
                if let Some(after) = after {
                    if after.index > self.project.pages[page_index].elements.len()
                        || self.elements.contains_key(&after.element.id)
                    {
                        return history("element index or ID");
                    }
                    self.project.pages[page_index]
                        .elements
                        .insert(after.index, after.element.clone());
                }
                self.reindex_page(page_index)?;
            }
        }
        Ok(())
    }
}

impl ChangeSet {
    fn for_batch(from: Revision, to: Revision, batch: &StoredBatch) -> Self {
        let mut pages = BTreeSet::new();
        let mut elements = BTreeSet::new();
        Self::record(batch, &mut pages, &mut elements);
        Self {
            from,
            to,
            pages: pages.into_iter().collect(),
            elements: elements.into_iter().collect(),
        }
    }

    fn add_batch(&mut self, batch: &StoredBatch) {
        let mut pages = self.pages.iter().copied().collect::<BTreeSet<_>>();
        let mut elements = self.elements.iter().copied().collect::<BTreeSet<_>>();
        Self::record(batch, &mut pages, &mut elements);
        self.pages = pages.into_iter().collect();
        self.elements = elements.into_iter().collect();
    }

    fn record(
        batch: &StoredBatch,
        pages: &mut BTreeSet<PageId>,
        elements: &mut BTreeSet<ElementId>,
    ) {
        for change in &batch.changes {
            match change {
                StoredChange::Page { before, after } => {
                    for positioned in [before, after].into_iter().flatten() {
                        pages.insert(positioned.page.id);
                        elements.extend(positioned.page.elements.iter().map(|element| element.id));
                    }
                }
                StoredChange::MovePage { page, .. }
                | StoredChange::PageName { page, .. }
                | StoredChange::PageSource { page, .. }
                | StoredChange::PageAsset { page, .. } => {
                    pages.insert(*page);
                }
                StoredChange::Element {
                    page,
                    before,
                    after,
                } => {
                    pages.insert(*page);
                    elements.extend(
                        [before, after]
                            .into_iter()
                            .flatten()
                            .map(|positioned| positioned.element.id),
                    );
                }
            }
        }
    }
}

fn apply_element_edit(element: &mut Element, edit: ElementChange) -> Result<()> {
    match edit {
        ElementChange::Frame(frame) => element.frame = frame,
        ElementChange::Visible(visible) => element.visible = visible,
        ElementChange::Opacity(opacity) => element.opacity = opacity,
        ElementChange::Source(source) => match &mut element.kind {
            ElementKind::Text(text) => text.source = source,
            ElementKind::Image(_) | ElementKind::Region(_) => {
                return Err(Error::ElementKind(element.id));
            }
        },
        ElementChange::Translation(translation) => match &mut element.kind {
            ElementKind::Text(text) => text.translation = translation,
            ElementKind::Image(_) | ElementKind::Region(_) => {
                return Err(Error::ElementKind(element.id));
            }
        },
        ElementChange::Style(style) => match &mut element.kind {
            ElementKind::Text(text) => text.style = style,
            ElementKind::Image(_) | ElementKind::Region(_) => {
                return Err(Error::ElementKind(element.id));
            }
        },
        ElementChange::Layout(layout) => match &mut element.kind {
            ElementKind::Text(text) => text.layout = layout,
            ElementKind::Image(_) | ElementKind::Region(_) => {
                return Err(Error::ElementKind(element.id));
            }
        },
        ElementChange::Analysis(analysis) => match &mut element.kind {
            ElementKind::Text(text) => text.set_analysis(analysis),
            ElementKind::Image(_) | ElementKind::Region(_) => {
                return Err(Error::ElementKind(element.id));
            }
        },
        ElementChange::Image { blob, natural_size } => match &mut element.kind {
            ElementKind::Image(image) => {
                image.blob = blob;
                image.natural_size = natural_size;
            }
            ElementKind::Text(_) | ElementKind::Region(_) => {
                return Err(Error::ElementKind(element.id));
            }
        },
        ElementChange::ImageName(name) => match &mut element.kind {
            ElementKind::Image(image) => image.name = name,
            ElementKind::Text(_) | ElementKind::Region(_) => {
                return Err(Error::ElementKind(element.id));
            }
        },
    }
    Ok(())
}

fn append_index(index: usize, len: usize) -> Result<usize> {
    if index == usize::MAX {
        Ok(len)
    } else if index <= len {
        Ok(index)
    } else {
        Err(Error::invalid("insertion index is out of range"))
    }
}

fn move_index(index: usize, len: usize) -> Result<usize> {
    if len == 0 {
        return Err(Error::invalid("cannot reorder an empty collection"));
    }
    if index == usize::MAX {
        Ok(len - 1)
    } else if index < len {
        Ok(index)
    } else {
        Err(Error::invalid("move index is out of range"))
    }
}

fn expect<T: PartialEq + ?Sized>(actual: &T, expected: &T, field: &str) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        history(field)
    }
}

fn history<T>(field: &str) -> Result<T> {
    Err(Error::HistoryConflict(field.to_owned()))
}

fn replay(
    connection: &rusqlite::Connection,
    state: &mut State,
    from: Revision,
    to: Revision,
) -> Result<()> {
    let mut ignored = ChangeSet::default();
    replay_collect(connection, state, from, to, &mut ignored)
}

fn replay_collect(
    connection: &rusqlite::Connection,
    state: &mut State,
    from: Revision,
    to: Revision,
    changes: &mut ChangeSet,
) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let mut applied = Vec::<StoredBatch>::new();
    let result = (|| -> Result<()> {
        let mut statement = connection.prepare_cached(
            "SELECT revision, parent_revision, changes FROM commits
             WHERE revision > ?1 AND revision <= ?2 ORDER BY revision",
        )?;
        let mut rows = statement.query(params![
            storage::revision_to_sql(from)?,
            storage::revision_to_sql(to)?,
        ])?;
        let mut expected_parent = from;
        while let Some(row) = rows.next()? {
            let revision = storage::revision_from_sql(row.get(0)?)?;
            let parent = storage::revision_from_sql(row.get(1)?)?;
            if parent != expected_parent || revision != parent.next().ok_or(Error::NotAProject)? {
                return Err(Error::NotAProject);
            }
            let bytes: Vec<u8> = row.get(2)?;
            let batch: StoredBatch = revision::from_slice(&bytes)?;
            state.apply_batch(&batch)?;
            changes.add_batch(&batch);
            applied.push(batch);
            expected_parent = revision;
        }
        if expected_parent != to {
            return Err(Error::NotAProject);
        }
        Ok(())
    })();
    if let Err(error) = result {
        for batch in applied.iter().rev() {
            state.apply_batch(&batch.reversed())?;
        }
        return Err(error);
    }
    Ok(())
}

fn current_blob_ids(project: &Project) -> HashSet<BlobId> {
    let mut ids = HashSet::new();
    project.blob_ids(&mut ids);
    ids
}

fn verify_blob_ids(
    connection: &rusqlite::Connection,
    ids: impl IntoIterator<Item = BlobId>,
) -> Result<()> {
    let mut statement = connection.prepare_cached("SELECT 1 FROM blobs WHERE id = ?1")?;
    for id in ids {
        if !statement.exists([id.as_bytes()])? {
            return Err(Error::invalid(format!("blob {id} was not found")));
        }
    }
    Ok(())
}

fn collect_garbage(connection: &rusqlite::Connection, project: &Project) -> Result<GcReport> {
    let mut reachable = current_blob_ids(project);
    {
        let mut statement = connection.prepare("SELECT changes FROM commits")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let bytes: Vec<u8> = row.get(0)?;
            revision::from_slice::<StoredBatch>(&bytes)?.blob_ids(&mut reachable);
        }
    }
    let mut garbage = Vec::new();
    {
        let mut statement = connection.prepare("SELECT id, length(bytes) FROM blobs")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let raw: Vec<u8> = row.get(0)?;
            let raw: [u8; 32] = raw.try_into().map_err(|_| Error::NotAProject)?;
            let id = BlobId::from_bytes(raw);
            if !reachable.contains(&id) {
                let bytes = u64::try_from(row.get::<_, i64>(1)?).map_err(|_| Error::NotAProject)?;
                garbage.push((id, bytes));
            }
        }
    }
    let mut delete = connection.prepare_cached("DELETE FROM blobs WHERE id = ?1")?;
    let mut report = GcReport::default();
    for (id, bytes) in garbage {
        report.blobs += delete.execute([id.as_bytes()])?;
        report.bytes += bytes;
    }
    Ok(report)
}
