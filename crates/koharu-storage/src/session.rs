use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    path::Path,
    sync::Arc,
    time::Duration,
};

use rusqlite::{Connection, MAIN_DB, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{
    BlobId, ChangeSet, DocumentId, Error, Patch, Result, Revision, Snapshot,
    blob::BlobStore,
    history::{Checkpoint, StoredCommit},
    patch::Operation,
    state::State,
    storage,
};

const MAX_DURABLE_ENVELOPE_BYTES: usize = 512 * 1024 * 1024;
const MAX_PATCH_OPERATIONS: usize = 10_000_000;

#[derive(Clone, Debug)]
pub struct Options {
    pub busy_timeout: Duration,
    pub checkpoint_commits: u64,
    pub checkpoint_bytes: u64,
    pub max_blob_bytes: usize,
    pub blob_cache_bytes: usize,
    pub blob_read_connections: usize,
    pub synchronous_full: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            busy_timeout: Duration::from_secs(5),
            checkpoint_commits: 1_024,
            checkpoint_bytes: 64 * 1024 * 1024,
            max_blob_bytes: 512 * 1024 * 1024,
            blob_cache_bytes: 256 * 1024 * 1024,
            blob_read_connections: 4,
            synchronous_full: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CommitResult {
    pub revision: Revision,
    pub changes: ChangeSet,
    pub snapshot: Snapshot,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct GcReport {
    pub blobs: usize,
    pub bytes: u64,
}

pub struct Session {
    connection: Connection,
    blobs: Arc<BlobStore>,
    state: Arc<State>,
    _state_lease: Arc<BTreeSet<BlobId>>,
    options: Options,
    _single_writer: PhantomData<Cell<()>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Session")
            .field("document", &self.state.document)
            .field("revision", &self.state.revision)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::create_with(path, Options::default())
    }

    pub fn create_with(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        validate_options(&options)?;
        let path = path.as_ref();
        if path.exists() {
            return Err(Error::invalid("storage document already exists"));
        }
        let connection =
            storage::create_disk(path, options.busy_timeout, options.synchronous_full)?;
        let state = State::empty(DocumentId::new());
        let checkpoint = revision::to_vec(&Checkpoint {
            document: state.to_checkpoint(),
        })?;
        storage::create_schema(&connection, state.document, &checkpoint)?;
        let blobs = BlobStore::file(
            path,
            options.busy_timeout,
            options.blob_cache_bytes,
            options.blob_read_connections,
        );
        Ok(Self::assemble(connection, blobs, state, options))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, Options::default())
    }

    pub fn open_with(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        validate_options(&options)?;
        let path = path.as_ref();
        let connection = storage::open_disk(path, options.busy_timeout, options.synchronous_full)?;
        let state = load_state(&connection)?;
        validate_blob_references(&connection, &state, &BTreeMap::new())?;
        let blobs = BlobStore::file(
            path,
            options.busy_timeout,
            options.blob_cache_bytes,
            options.blob_read_connections,
        );
        Ok(Self::assemble(connection, blobs, state, options))
    }

    pub fn memory() -> Result<Self> {
        Self::memory_with(Options::default())
    }

    pub fn memory_with(options: Options) -> Result<Self> {
        validate_options(&options)?;
        let (connection, uri) =
            storage::open_memory(options.busy_timeout, options.synchronous_full)?;
        let state = State::empty(DocumentId::new());
        let checkpoint = revision::to_vec(&Checkpoint {
            document: state.to_checkpoint(),
        })?;
        storage::create_schema(&connection, state.document, &checkpoint)?;
        let blobs = BlobStore::uri(
            uri,
            options.busy_timeout,
            options.blob_cache_bytes,
            options.blob_read_connections,
        );
        Ok(Self::assemble(connection, blobs, state, options))
    }

    fn assemble(
        connection: Connection,
        blobs: Arc<BlobStore>,
        state: State,
        options: Options,
    ) -> Self {
        let state = Arc::new(state);
        let state_lease = blobs.lease(state.referenced_blobs());
        Self {
            connection,
            blobs,
            state,
            _state_lease: state_lease,
            options,
            _single_writer: PhantomData,
        }
    }

    #[must_use]
    pub fn document_id(&self) -> DocumentId {
        self.state.document
    }

    #[must_use]
    pub fn revision(&self) -> Revision {
        self.state.revision
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot::new(self.state.clone(), self.blobs.clone())
    }

    pub fn commit(&mut self, patch: Patch) -> Result<CommitResult> {
        if patch.base().document != self.document_id() {
            return Err(Error::DocumentMismatch {
                patch: patch.base().document,
                session: self.document_id(),
            });
        }
        if patch.base().revision != self.revision() {
            return Err(Error::RevisionConflict {
                expected: patch.base().revision,
                actual: self.revision(),
            });
        }
        if patch.is_empty() {
            let actual = storage::head(&self.connection)?;
            if actual != self.revision() {
                return Err(Error::RevisionConflict {
                    expected: self.revision(),
                    actual,
                });
            }
            let revision = self.revision();
            return Ok(CommitResult {
                revision,
                changes: ChangeSet::empty(revision),
                snapshot: self.snapshot(),
            });
        }

        let (mut next, attachments) = patch.apply(&self.state)?;
        validate_attachments(&attachments, self.options.max_blob_bytes)?;
        let operations = patch.operations().cloned().collect::<Vec<_>>();
        let operation_blobs = operation_blob_ids(&operations);
        validate_blob_ids(&self.connection, &operation_blobs, &attachments)?;

        let parent = self.revision();
        let revision = parent
            .next()
            .ok_or_else(|| Error::invalid("document revision overflow"))?;
        if operations.len() > MAX_PATCH_OPERATIONS {
            return Err(Error::invalid("patch contains too many operations"));
        }
        let stored = StoredCommit {
            label: patch.label().map(str::to_owned),
            operations: operations.clone(),
        };
        let payload = revision::to_vec(&stored)?;
        check_envelope_size(&payload, "commit")?;
        let meta = storage::meta(&self.connection)?;
        let commit_count = meta.commits_since_checkpoint.saturating_add(1);
        let commit_bytes = meta
            .bytes_since_checkpoint
            .saturating_add(payload.len() as u64);
        next.revision = revision;
        let make_checkpoint = threshold_reached(commit_count, self.options.checkpoint_commits)
            || threshold_reached(commit_bytes, self.options.checkpoint_bytes);
        let checkpoint = make_checkpoint
            .then(|| {
                revision::to_vec(&Checkpoint {
                    document: next.to_checkpoint(),
                })
            })
            .transpose()?;
        if let Some(checkpoint) = &checkpoint {
            check_envelope_size(checkpoint, "checkpoint")?;
        }
        let new_blobs = attachments
            .keys()
            .filter(|id| !blob_exists(&self.connection, **id).unwrap_or(false))
            .copied()
            .collect::<Vec<_>>();
        let changes = ChangeSet::from_operations(
            parent,
            revision,
            operations.iter(),
            new_blobs.iter().copied(),
        );

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_head(&transaction, parent)?;
        persist_blobs(&transaction, &operation_blobs, &attachments)?;
        transaction.execute(
            "INSERT INTO commits (revision, parent_revision, label, payload)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                storage::revision_to_sql(revision)?,
                storage::revision_to_sql(parent)?,
                stored.label.as_deref(),
                payload,
            ],
        )?;
        for id in &operation_blobs {
            transaction.execute(
                "INSERT INTO commit_blobs (revision, blob_id) VALUES (?1, ?2)",
                params![
                    storage::revision_to_sql(revision)?,
                    id.as_bytes().as_slice()
                ],
            )?;
        }
        if let Some(checkpoint) = checkpoint {
            transaction.execute(
                "UPDATE meta SET head_revision = ?1, checkpoint_revision = ?1,
                 checkpoint = ?2, commits_since_checkpoint = 0,
                 bytes_since_checkpoint = 0 WHERE singleton = 1",
                params![storage::revision_to_sql(revision)?, checkpoint],
            )?;
        } else {
            transaction.execute(
                "UPDATE meta SET head_revision = ?1, commits_since_checkpoint = ?2,
                 bytes_since_checkpoint = ?3 WHERE singleton = 1",
                params![
                    storage::revision_to_sql(revision)?,
                    sql_u64(commit_count)?,
                    sql_u64(commit_bytes)?,
                ],
            )?;
        }
        transaction.commit()?;
        let state_lease = self.blobs.lease(next.referenced_blobs());
        self.state = Arc::new(next);
        self._state_lease = state_lease;
        Ok(CommitResult {
            revision,
            changes,
            snapshot: self.snapshot(),
        })
    }

    pub fn refresh(&mut self) -> Result<ChangeSet> {
        let before = self.state.clone();
        let head = storage::head(&self.connection)?;
        if head == before.revision {
            return Ok(ChangeSet::empty(before.revision));
        }
        if head < before.revision {
            return Err(Error::NotADocument);
        }
        let meta = storage::meta(&self.connection)?;
        let (next, changes) = if let Some(result) = load_tail(&self.connection, &before, meta.head)?
        {
            result
        } else {
            let next = load_state(&self.connection)?;
            let changes = ChangeSet::between(&before, &next, []);
            (next, changes)
        };
        validate_blob_references(&self.connection, &next, &BTreeMap::new())?;
        let state_lease = self.blobs.lease(next.referenced_blobs());
        self.state = Arc::new(next);
        self._state_lease = state_lease;
        Ok(changes)
    }

    pub fn undo(&mut self, revision: Revision) -> Result<CommitResult> {
        self.undo_many([revision])
    }

    pub fn undo_many(
        &mut self,
        revisions: impl IntoIterator<Item = Revision>,
    ) -> Result<CommitResult> {
        let mut revisions = revisions.into_iter().collect::<Vec<_>>();
        revisions.sort_unstable_by(|left, right| right.cmp(left));
        revisions.dedup();
        if revisions.is_empty() {
            return Err(Error::invalid("undo requires at least one revision"));
        }
        let mut operations = Vec::new();
        for revision in &revisions {
            let payload = self
                .connection
                .query_row(
                    "SELECT payload FROM commits WHERE revision = ?1",
                    [storage::revision_to_sql(*revision)?],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()?
                .ok_or(Error::HistoryNotFound(*revision))?;
            let commit: StoredCommit = revision::from_slice(&payload)?;
            operations.extend(commit.operations.iter().rev().map(Operation::reversed));
        }
        let label: Arc<str> = if revisions.len() == 1 {
            format!("Undo revision {}", revisions[0]).into()
        } else {
            format!("Undo {} revisions", revisions.len()).into()
        };
        let patch = Patch::from_operations(self.state.clone(), operations, Some(label))?;
        self.commit(patch)
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        let revision = self.revision();
        let checkpoint = revision::to_vec(&Checkpoint {
            document: self.state.to_checkpoint(),
        })?;
        check_envelope_size(&checkpoint, "checkpoint")?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_head(&transaction, revision)?;
        transaction.execute(
            "UPDATE meta SET checkpoint_revision = head_revision, checkpoint = ?1,
             commits_since_checkpoint = 0, bytes_since_checkpoint = 0 WHERE singleton = 1",
            [checkpoint],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn prune_history(&mut self, keep_from: Revision) -> Result<GcReport> {
        if keep_from > self.revision().next().unwrap_or(self.revision()) {
            return Err(Error::invalid("history retention begins after the head"));
        }
        self.checkpoint()?;
        let revision = self.revision();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_head(&transaction, revision)?;
        transaction.execute(
            "DELETE FROM commits WHERE revision < ?1",
            [storage::revision_to_sql(keep_from)?],
        )?;
        let (report, removed) =
            collect_garbage(&transaction, &self.state, self.blobs.live_blobs())?;
        transaction.commit()?;
        self.blobs.invalidate(&removed);
        Ok(report)
    }

    pub fn gc(&mut self) -> Result<GcReport> {
        let revision = self.revision();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_head(&transaction, revision)?;
        let (report, removed) =
            collect_garbage(&transaction, &self.state, self.blobs.live_blobs())?;
        transaction.commit()?;
        self.blobs.invalidate(&removed);
        Ok(report)
    }

    pub fn compact(&mut self, keep_from: Revision) -> Result<GcReport> {
        let report = self.prune_history(keep_from)?;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        Ok(report)
    }

    pub fn backup(&self, path: impl AsRef<Path>) -> Result<()> {
        self.connection.backup(MAIN_DB, path.as_ref(), None)?;
        Ok(())
    }
}

fn load_state(connection: &Connection) -> Result<State> {
    let meta = storage::meta(connection)?;
    if meta.checkpoint_revision > meta.head {
        return Err(Error::NotADocument);
    }
    check_envelope_size(&meta.checkpoint, "checkpoint")?;
    let checkpoint: Checkpoint = revision::from_slice(&meta.checkpoint)?;
    let mut state =
        State::from_checkpoint(meta.document, meta.checkpoint_revision, checkpoint.document)?;
    let mut statement = connection.prepare(
        "SELECT revision, parent_revision, payload FROM commits
         WHERE revision > ?1 AND revision <= ?2 ORDER BY revision",
    )?;
    let mut rows = statement.query(params![
        storage::revision_to_sql(meta.checkpoint_revision)?,
        storage::revision_to_sql(meta.head)?,
    ])?;
    let mut expected_parent = meta.checkpoint_revision;
    while let Some(row) = rows.next()? {
        let revision = storage::revision_from_sql(row.get(0)?)?;
        let parent = storage::revision_from_sql(row.get(1)?)?;
        if parent != expected_parent || revision != parent.next().ok_or(Error::NotADocument)? {
            return Err(Error::NotADocument);
        }
        let payload: Vec<u8> = row.get(2)?;
        check_envelope_size(&payload, "commit")?;
        let commit: StoredCommit = revision::from_slice(&payload)?;
        for operation in &commit.operations {
            operation
                .apply(&mut state)
                .map_err(|error| Error::HistoryConflict(format!("revision {revision}: {error}")))?;
        }
        state.revision = revision;
        expected_parent = revision;
    }
    if state.revision != meta.head {
        return Err(Error::NotADocument);
    }
    state.validate()?;
    Ok(state)
}

fn load_tail(
    connection: &Connection,
    current: &State,
    head: Revision,
) -> Result<Option<(State, ChangeSet)>> {
    let mut statement = connection.prepare(
        "SELECT revision, parent_revision, payload FROM commits
         WHERE revision > ?1 AND revision <= ?2 ORDER BY revision",
    )?;
    let mut rows = statement.query(params![
        storage::revision_to_sql(current.revision)?,
        storage::revision_to_sql(head)?,
    ])?;
    let mut next = current.clone();
    let mut expected_parent = current.revision;
    let mut saw_commit = false;
    let mut operations = Vec::new();
    while let Some(row) = rows.next()? {
        saw_commit = true;
        let revision = storage::revision_from_sql(row.get(0)?)?;
        let parent = storage::revision_from_sql(row.get(1)?)?;
        if parent != expected_parent || revision != parent.next().ok_or(Error::NotADocument)? {
            return Ok(None);
        }
        let payload: Vec<u8> = row.get(2)?;
        check_envelope_size(&payload, "commit")?;
        let commit: StoredCommit = revision::from_slice(&payload)?;
        for operation in &commit.operations {
            operation
                .apply(&mut next)
                .map_err(|error| Error::HistoryConflict(format!("revision {revision}: {error}")))?;
        }
        operations.extend(commit.operations);
        next.revision = revision;
        expected_parent = revision;
    }
    if saw_commit && next.revision == head {
        next.validate()?;
        let changes = ChangeSet::from_operations(current.revision, head, operations.iter(), []);
        Ok(Some((next, changes)))
    } else {
        Ok(None)
    }
}

fn validate_options(options: &Options) -> Result<()> {
    if options.max_blob_bytes == 0 {
        return Err(Error::invalid("maximum blob size must be non-zero"));
    }
    if options.blob_read_connections == 0 {
        return Err(Error::invalid(
            "blob reader connection count must be non-zero",
        ));
    }
    Ok(())
}

fn validate_attachments(
    attachments: &BTreeMap<BlobId, Arc<[u8]>>,
    max_blob_bytes: usize,
) -> Result<()> {
    for (id, bytes) in attachments {
        if bytes.len() > max_blob_bytes {
            return Err(Error::invalid(format!(
                "blob {id} exceeds the configured size limit"
            )));
        }
        if BlobId::for_bytes(bytes) != *id {
            return Err(Error::invalid(format!("blob {id} has an invalid hash")));
        }
    }
    Ok(())
}

fn validate_blob_references(
    connection: &Connection,
    state: &State,
    attachments: &BTreeMap<BlobId, Arc<[u8]>>,
) -> Result<()> {
    for id in state.referenced_blobs() {
        if !attachments.contains_key(&id) && !blob_exists(connection, id)? {
            return Err(Error::BlobNotFound(id));
        }
    }
    Ok(())
}

fn validate_blob_ids(
    connection: &Connection,
    ids: &BTreeSet<BlobId>,
    attachments: &BTreeMap<BlobId, Arc<[u8]>>,
) -> Result<()> {
    for id in ids {
        if !attachments.contains_key(id) && !blob_exists(connection, *id)? {
            return Err(Error::BlobNotFound(*id));
        }
    }
    Ok(())
}

fn blob_exists(connection: &Connection, id: BlobId) -> Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM blobs WHERE id = ?1",
            [id.as_bytes().as_slice()],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(Into::into)
}

fn operation_blob_ids(operations: &[Operation]) -> BTreeSet<BlobId> {
    let mut result = BTreeSet::new();
    for operation in operations {
        operation.blob_refs(&mut result);
    }
    result
}

fn persist_blobs(
    transaction: &Transaction<'_>,
    referenced: &BTreeSet<BlobId>,
    attachments: &BTreeMap<BlobId, Arc<[u8]>>,
) -> Result<()> {
    for id in referenced {
        if blob_exists(transaction, *id)? {
            continue;
        }
        let bytes = attachments.get(id).ok_or(Error::BlobNotFound(*id))?;
        transaction.execute(
            "INSERT INTO blobs (id, byte_len, bytes) VALUES (?1, ?2, ?3)",
            params![
                id.as_bytes().as_slice(),
                i64::try_from(bytes.len()).map_err(|_| Error::invalid("blob is too large"))?,
                bytes.as_ref(),
            ],
        )?;
    }
    Ok(())
}

fn collect_garbage(
    transaction: &Transaction<'_>,
    state: &State,
    live: BTreeSet<BlobId>,
) -> Result<(GcReport, BTreeSet<BlobId>)> {
    let mut marked = state.referenced_blobs();
    marked.extend(live);
    let mut statement = transaction.prepare("SELECT DISTINCT blob_id FROM commit_blobs")?;
    let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
    for row in rows {
        marked.insert(blob_id_from_sql(&row?)?);
    }
    drop(statement);

    let mut statement = transaction.prepare("SELECT id, byte_len FROM blobs")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut removed = BTreeSet::new();
    let mut bytes = 0_u64;
    for row in rows {
        let (id, byte_len) = row?;
        let id = blob_id_from_sql(&id)?;
        if !marked.contains(&id) {
            removed.insert(id);
            bytes = bytes.saturating_add(u64::try_from(byte_len).map_err(|_| Error::NotADocument)?);
        }
    }
    drop(statement);
    for id in &removed {
        transaction.execute(
            "DELETE FROM blobs WHERE id = ?1",
            [id.as_bytes().as_slice()],
        )?;
    }
    Ok((
        GcReport {
            blobs: removed.len(),
            bytes,
        },
        removed,
    ))
}

fn blob_id_from_sql(bytes: &[u8]) -> Result<BlobId> {
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| Error::NotADocument)?;
    Ok(BlobId::from_bytes(bytes))
}

fn ensure_head(connection: &Connection, expected: Revision) -> Result<()> {
    let actual = storage::head(connection)?;
    if actual == expected {
        Ok(())
    } else {
        Err(Error::RevisionConflict { expected, actual })
    }
}

fn threshold_reached(value: u64, threshold: u64) -> bool {
    threshold != 0 && value >= threshold
}

fn sql_u64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::invalid("counter exceeds SQLite INTEGER"))
}

fn check_envelope_size(bytes: &[u8], name: &str) -> Result<()> {
    if bytes.len() > MAX_DURABLE_ENVELOPE_BYTES {
        Err(Error::invalid(format!(
            "{name} exceeds the durable size limit"
        )))
    } else {
        Ok(())
    }
}
