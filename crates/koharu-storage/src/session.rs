use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    path::Path,
    sync::Arc,
};

use redb::{ReadableTable, WriteTransaction};

use crate::{
    BlobId, Commit, CommitRequest, DocumentId, Error, Recovery, Refresh, Result, Revision,
    Snapshot,
    history::StoredCommit,
    storage::{BLOBS, COMMITS, FORMAT_VERSION, META, Metadata, Store},
};

const MAX_DURABLE_ENVELOPE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Options {
    pub database_cache_bytes: usize,
    pub blob_cache_bytes: usize,
    pub max_blob_bytes: usize,
    pub checkpoint_commits: u64,
    pub checkpoint_bytes: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            database_cache_bytes: 64 * 1024 * 1024,
            blob_cache_bytes: 256 * 1024 * 1024,
            max_blob_bytes: 512 * 1024 * 1024,
            checkpoint_commits: 1_024,
            checkpoint_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CommitResult {
    pub revision: Revision,
    pub snapshot: Snapshot,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct GcReport {
    pub blobs: usize,
    pub bytes: u64,
}

/// The single writer for one durable document. It tracks only the durable
/// revision; the interpreted document state remains owned by the caller.
pub struct Session {
    store: Arc<Store>,
    document: DocumentId,
    revision: Revision,
    options: Options,
    _single_writer: PhantomData<Cell<()>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Session")
            .field("document", &self.document)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn create(
        path: impl AsRef<Path>,
        document: DocumentId,
        checkpoint: Vec<u8>,
    ) -> Result<Self> {
        Self::create_with(path, document, checkpoint, Options::default())
    }

    pub fn create_with(
        path: impl AsRef<Path>,
        document: DocumentId,
        checkpoint: Vec<u8>,
        options: Options,
    ) -> Result<Self> {
        validate_options(&options)?;
        check_envelope_size(&checkpoint, "checkpoint")?;
        let store = Store::create(
            path.as_ref(),
            options.database_cache_bytes,
            options.blob_cache_bytes,
        )?;
        store.initialize(&Metadata {
            format: FORMAT_VERSION,
            document,
            head: Revision::ZERO,
            checkpoint_revision: Revision::ZERO,
            checkpoint,
            commits_since_checkpoint: 0,
            bytes_since_checkpoint: 0,
        })?;
        Ok(Self::assemble(store, document, Revision::ZERO, options))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, Options::default())
    }

    pub fn open_with(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        validate_options(&options)?;
        let store = Store::open(
            path.as_ref(),
            options.database_cache_bytes,
            options.blob_cache_bytes,
        )?;
        let metadata = store.metadata()?;
        Ok(Self::assemble(
            store,
            metadata.document,
            metadata.head,
            options,
        ))
    }

    pub fn memory(document: DocumentId, checkpoint: Vec<u8>) -> Result<Self> {
        Self::memory_with(document, checkpoint, Options::default())
    }

    pub fn memory_with(
        document: DocumentId,
        checkpoint: Vec<u8>,
        options: Options,
    ) -> Result<Self> {
        validate_options(&options)?;
        check_envelope_size(&checkpoint, "checkpoint")?;
        let store = Store::memory(options.database_cache_bytes, options.blob_cache_bytes)?;
        store.initialize(&Metadata {
            format: FORMAT_VERSION,
            document,
            head: Revision::ZERO,
            checkpoint_revision: Revision::ZERO,
            checkpoint,
            commits_since_checkpoint: 0,
            bytes_since_checkpoint: 0,
        })?;
        Ok(Self::assemble(store, document, Revision::ZERO, options))
    }

    fn assemble(
        store: Arc<Store>,
        document: DocumentId,
        revision: Revision,
        options: Options,
    ) -> Self {
        Self {
            store,
            document,
            revision,
            options,
            _single_writer: PhantomData,
        }
    }

    #[must_use]
    pub const fn document_id(&self) -> DocumentId {
        self.document
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub fn snapshot(&self, referenced: BTreeSet<BlobId>) -> Snapshot {
        Snapshot::new(self.document, self.revision, self.store.clone(), referenced)
    }

    pub fn recovery(&self) -> Result<Recovery> {
        let metadata = self.store.metadata()?;
        if metadata.document != self.document
            || metadata.checkpoint_revision > metadata.head
            || metadata.head != self.revision
        {
            return Err(Error::NotADocument);
        }
        check_envelope_size(&metadata.checkpoint, "checkpoint")?;
        let commits = decode_commits(
            self.store
                .load_commits(metadata.checkpoint_revision, metadata.head)?,
        )?;
        Ok(Recovery {
            document: self.document,
            checkpoint_revision: metadata.checkpoint_revision,
            head: metadata.head,
            checkpoint: Arc::from(metadata.checkpoint),
            commits,
        })
    }

    pub fn checkpoint_due(&self, commit_bytes: usize) -> Result<bool> {
        let metadata = self.store.metadata()?;
        ensure_head(&metadata, self.revision)?;
        Ok(threshold_reached(
            metadata.commits_since_checkpoint.saturating_add(1),
            self.options.checkpoint_commits,
        ) || threshold_reached(
            metadata
                .bytes_since_checkpoint
                .saturating_add(commit_bytes as u64),
            self.options.checkpoint_bytes,
        ))
    }

    pub fn commit(
        &mut self,
        request: CommitRequest,
        checkpoint: Option<Vec<u8>>,
        referenced: BTreeSet<BlobId>,
    ) -> Result<CommitResult> {
        if request.document != self.document {
            return Err(Error::DocumentMismatch {
                patch: request.document,
                session: self.document,
            });
        }
        if request.parent != self.revision {
            return Err(Error::RevisionConflict {
                expected: request.parent,
                actual: self.revision,
            });
        }
        check_envelope_size(&request.forward, "forward commit")?;
        check_envelope_size(&request.inverse, "inverse commit")?;
        if let Some(checkpoint) = &checkpoint {
            check_envelope_size(checkpoint, "checkpoint")?;
        }

        let revision = self
            .revision
            .next()
            .ok_or_else(|| Error::invalid("document revision overflow"))?;
        let mut attachments = BTreeMap::new();
        for attachment in request.attachments {
            let bytes = attachment.bytes();
            if bytes.len() > self.options.max_blob_bytes {
                return Err(Error::invalid(format!(
                    "blob {} exceeds the configured size limit",
                    attachment.id()
                )));
            }
            attachments.insert(attachment.id(), bytes);
        }
        let stored = StoredCommit {
            label: request.label.as_deref().map(str::to_owned),
            forward: request.forward,
            inverse: request.inverse,
            blobs: request.blobs.iter().copied().collect(),
        };
        let payload = revision::to_vec(&stored)?;
        check_envelope_size(&payload, "commit")?;

        let transaction = self.store.write()?;
        let mut metadata = metadata_in(&transaction)?;
        ensure_head(&metadata, self.revision)?;
        let commit_count = metadata.commits_since_checkpoint.saturating_add(1);
        let commit_bytes = metadata
            .bytes_since_checkpoint
            .saturating_add(payload.len() as u64);
        let make_checkpoint = threshold_reached(commit_count, self.options.checkpoint_commits)
            || threshold_reached(commit_bytes, self.options.checkpoint_bytes);
        if make_checkpoint && checkpoint.is_none() {
            return Err(Error::invalid(
                "a checkpoint is required after the configured commit threshold",
            ));
        }

        {
            let mut blobs = transaction.open_table(BLOBS)?;
            for id in &request.blobs {
                if blobs.get(*id)?.is_some() {
                    continue;
                }
                let bytes = attachments.get(id).ok_or(Error::BlobNotFound(*id))?;
                blobs.insert(*id, bytes.as_ref())?;
            }
        }
        {
            let mut commits = transaction.open_table(COMMITS)?;
            if commits
                .insert(revision.get(), payload.as_slice())?
                .is_some()
            {
                return Err(Error::NotADocument);
            }
        }
        metadata.head = revision;
        if make_checkpoint {
            metadata.checkpoint_revision = revision;
            metadata.checkpoint = checkpoint.expect("checkpoint checked above");
            metadata.commits_since_checkpoint = 0;
            metadata.bytes_since_checkpoint = 0;
        } else {
            metadata.commits_since_checkpoint = commit_count;
            metadata.bytes_since_checkpoint = commit_bytes;
        }
        write_metadata(&transaction, &metadata)?;
        transaction.commit()?;

        self.revision = revision;
        Ok(CommitResult {
            revision,
            snapshot: self.snapshot(referenced),
        })
    }

    pub fn prepare_refresh(&self) -> Result<Refresh> {
        let metadata = self.store.metadata()?;
        if metadata.document != self.document || metadata.head < self.revision {
            return Err(Error::NotADocument);
        }
        let commits = decode_commits(self.store.load_commits(self.revision, metadata.head)?)?;
        Ok(Refresh {
            from: self.revision,
            to: metadata.head,
            commits,
        })
    }

    pub fn accept_refresh(&mut self, refresh: &Refresh) -> Result<()> {
        if refresh.from != self.revision
            || refresh
                .commits
                .last()
                .map_or(refresh.from, |commit| commit.revision)
                != refresh.to
        {
            return Err(Error::RevisionConflict {
                expected: refresh.from,
                actual: self.revision,
            });
        }
        self.revision = refresh.to;
        Ok(())
    }

    pub fn history(&self, revision: Revision) -> Result<Commit> {
        let payload = self
            .store
            .load_commit(revision)?
            .ok_or(Error::HistoryNotFound(revision))?;
        decode_commit(revision, &payload)
    }

    pub fn checkpoint(&mut self, checkpoint: Vec<u8>) -> Result<()> {
        check_envelope_size(&checkpoint, "checkpoint")?;
        let transaction = self.store.write()?;
        let mut metadata = metadata_in(&transaction)?;
        ensure_head(&metadata, self.revision)?;
        metadata.checkpoint_revision = self.revision;
        metadata.checkpoint = checkpoint;
        metadata.commits_since_checkpoint = 0;
        metadata.bytes_since_checkpoint = 0;
        write_metadata(&transaction, &metadata)?;
        transaction.commit()?;
        self.store.flush()
    }

    pub fn prune_history(
        &mut self,
        keep_from: Revision,
        checkpoint: Vec<u8>,
        referenced: BTreeSet<BlobId>,
    ) -> Result<GcReport> {
        if keep_from > self.revision.next().unwrap_or(self.revision) {
            return Err(Error::invalid("history retention begins after the head"));
        }
        self.checkpoint(checkpoint)?;
        let transaction = self.store.write()?;
        let metadata = metadata_in(&transaction)?;
        ensure_head(&metadata, self.revision)?;
        {
            let mut commits = transaction.open_table(COMMITS)?;
            let keys = commits
                .range(..keep_from.get())?
                .map(|entry| entry.map(|(key, _)| key.value()))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for key in keys {
                commits.remove(key)?;
            }
        }
        let (report, removed) = collect_garbage(&transaction, referenced, self.store.live_blobs())?;
        transaction.commit()?;
        self.store.flush()?;
        self.store.invalidate_blobs(&removed);
        Ok(report)
    }

    pub fn gc(&mut self, referenced: BTreeSet<BlobId>) -> Result<GcReport> {
        let transaction = self.store.write()?;
        let metadata = metadata_in(&transaction)?;
        ensure_head(&metadata, self.revision)?;
        let (report, removed) = collect_garbage(&transaction, referenced, self.store.live_blobs())?;
        transaction.commit()?;
        self.store.flush()?;
        self.store.invalidate_blobs(&removed);
        Ok(report)
    }

    pub fn compact(
        &mut self,
        keep_from: Revision,
        checkpoint: Vec<u8>,
        referenced: BTreeSet<BlobId>,
    ) -> Result<GcReport> {
        let report = self.prune_history(keep_from, checkpoint, referenced)?;
        self.store.compact()?;
        Ok(report)
    }

    pub fn backup(&self, path: impl AsRef<Path>) -> Result<()> {
        self.store
            .backup(path.as_ref(), self.options.database_cache_bytes)
    }

    pub fn flush(&self) -> Result<()> {
        self.store.flush()
    }
}

fn decode_commits(values: Vec<(Revision, Vec<u8>)>) -> Result<Vec<Commit>> {
    values
        .into_iter()
        .map(|(revision, payload)| decode_commit(revision, &payload))
        .collect()
}

fn decode_commit(revision: Revision, payload: &[u8]) -> Result<Commit> {
    check_envelope_size(payload, "commit")?;
    let stored: StoredCommit = revision::from_slice(payload)?;
    Ok(stored.into_commit(revision))
}

fn collect_garbage(
    transaction: &WriteTransaction,
    mut marked: BTreeSet<BlobId>,
    live: BTreeSet<BlobId>,
) -> Result<(GcReport, BTreeSet<BlobId>)> {
    marked.extend(live);
    {
        let commits = transaction.open_table(COMMITS)?;
        for entry in commits.iter()? {
            let (_, payload) = entry?;
            let commit: StoredCommit = revision::from_slice(payload.value())?;
            marked.extend(commit.blobs);
        }
    }

    let mut removed = BTreeSet::new();
    let mut bytes = 0_u64;
    {
        let mut blobs = transaction.open_table(BLOBS)?;
        for entry in blobs.iter()? {
            let (id, value) = entry?;
            let id = id.value();
            if !marked.contains(&id) {
                removed.insert(id);
                bytes = bytes.saturating_add(value.value().len() as u64);
            }
        }
        for id in &removed {
            blobs.remove(*id)?;
        }
    }
    Ok((
        GcReport {
            blobs: removed.len(),
            bytes,
        },
        removed,
    ))
}

fn metadata_in(transaction: &WriteTransaction) -> Result<Metadata> {
    let meta = transaction.open_table(META)?;
    let encoded = meta.get(0)?.ok_or(Error::NotADocument)?;
    let metadata: Metadata = revision::from_slice(encoded.value())?;
    if metadata.format != FORMAT_VERSION {
        return Err(Error::UnsupportedSchema(metadata.format));
    }
    Ok(metadata)
}

fn write_metadata(transaction: &WriteTransaction, metadata: &Metadata) -> Result<()> {
    let encoded = revision::to_vec(metadata)?;
    transaction
        .open_table(META)?
        .insert(0, encoded.as_slice())?;
    Ok(())
}

fn ensure_head(metadata: &Metadata, expected: Revision) -> Result<()> {
    if metadata.head == expected {
        Ok(())
    } else {
        Err(Error::RevisionConflict {
            expected,
            actual: metadata.head,
        })
    }
}

fn validate_options(options: &Options) -> Result<()> {
    if options.database_cache_bytes == 0 {
        return Err(Error::invalid("database cache size must be non-zero"));
    }
    if options.max_blob_bytes == 0 {
        return Err(Error::invalid("maximum blob size must be non-zero"));
    }
    Ok(())
}

fn threshold_reached(value: u64, threshold: u64) -> bool {
    threshold != 0 && value >= threshold
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
