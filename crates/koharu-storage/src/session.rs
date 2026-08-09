use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    path::Path,
    sync::Arc,
};

use crate::{
    BlobId, Commit, DocumentId, Error, HistoryEntry, Recovery, Refresh, Result, Revision, Snapshot,
    commit::StoredEntry,
    database::{Engine, Head},
};

const MAX_RECORD_BYTES: usize = 512 * 1024 * 1024;

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

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct GcReport {
    pub blobs: usize,
    pub bytes: u64,
}

/// One document owner. RocksDB serializes its atomic batches; the Rust type is
/// deliberately not `Sync` so application code cannot accidentally create a
/// second writer around the same session value.
pub struct Session {
    engine: Arc<Engine>,
    head: Head,
    options: Options,
    _single_writer: PhantomData<Cell<()>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Session")
            .field("document", &self.head.document)
            .field("revision", &self.head.revision)
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

    #[tracing::instrument(
        skip_all,
        fields(path = %path.as_ref().display(), checkpoint_bytes = checkpoint.len())
    )]
    pub fn create_with(
        path: impl AsRef<Path>,
        document: DocumentId,
        checkpoint: Vec<u8>,
        options: Options,
    ) -> Result<Self> {
        validate_options(&options)?;
        validate_size(&checkpoint, "checkpoint")?;
        let engine = Engine::create(
            path.as_ref(),
            options.database_cache_bytes,
            options.blob_cache_bytes,
        )?;
        let head = Head::empty(document);
        engine.initialize(&head, &checkpoint)?;
        Ok(Self::new(engine, head, options))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, Options::default())
    }

    #[tracing::instrument(skip_all, fields(path = %path.as_ref().display()))]
    pub fn open_with(path: impl AsRef<Path>, options: Options) -> Result<Self> {
        validate_options(&options)?;
        let engine = Engine::open(
            path.as_ref(),
            options.database_cache_bytes,
            options.blob_cache_bytes,
        )?;
        let head = engine.head()?;
        Ok(Self::new(engine, head, options))
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
        validate_size(&checkpoint, "checkpoint")?;
        let engine = Engine::memory(options.database_cache_bytes, options.blob_cache_bytes)?;
        let head = Head::empty(document);
        engine.initialize(&head, &checkpoint)?;
        Ok(Self::new(engine, head, options))
    }

    fn new(engine: Arc<Engine>, head: Head, options: Options) -> Self {
        Self {
            engine,
            head,
            options,
            _single_writer: PhantomData,
        }
    }

    #[must_use]
    pub const fn document_id(&self) -> DocumentId {
        self.head.document
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.head.revision
    }

    pub fn snapshot(&self, referenced: BTreeSet<BlobId>) -> Result<Snapshot> {
        Snapshot::durable(
            self.head.document,
            self.head.revision,
            self.engine.clone(),
            referenced,
        )
    }

    pub fn recover(&self) -> Result<Recovery> {
        let (head, checkpoint, entries) = self.engine.recovery()?;
        if head.document != self.head.document || head.revision != self.head.revision {
            return Err(Error::NotAProject);
        }
        validate_size(&checkpoint, "checkpoint")?;
        Ok(Recovery {
            document: head.document,
            checkpoint_revision: head.checkpoint_revision,
            head: head.revision,
            checkpoint: Arc::from(checkpoint),
            entries: decode_entries(entries)?,
        })
    }

    #[must_use]
    pub fn needs_checkpoint(&self, commit_bytes: usize) -> bool {
        reached(
            self.head.commits_since_checkpoint.saturating_add(1),
            self.options.checkpoint_commits,
        ) || reached(
            self.head
                .bytes_since_checkpoint
                .saturating_add(commit_bytes as u64),
            self.options.checkpoint_bytes,
        )
    }

    #[tracing::instrument(
        skip_all,
        fields(
            document = %commit.document,
            parent = %commit.parent,
            payload_bytes = commit.payload_len(),
            checkpoint = checkpoint.is_some(),
        )
    )]
    pub fn commit(&mut self, commit: Commit, checkpoint: Option<Vec<u8>>) -> Result<Revision> {
        if commit.document != self.head.document {
            return Err(Error::DocumentMismatch {
                patch: commit.document,
                session: self.head.document,
            });
        }
        if commit.parent != self.head.revision {
            return Err(Error::RevisionConflict {
                expected: commit.parent,
                actual: self.head.revision,
            });
        }
        validate_size(&commit.forward, "forward history")?;
        validate_size(&commit.inverse, "inverse history")?;
        if let Some(checkpoint) = &checkpoint {
            validate_size(checkpoint, "checkpoint")?;
        }
        let commit_bytes = commit.payload_len();
        let checkpoint_required = self.needs_checkpoint(commit_bytes);
        if checkpoint_required && checkpoint.is_none() {
            return Err(Error::invalid(
                "checkpoint required by the configured threshold",
            ));
        }

        let revision = self
            .head
            .revision
            .next()
            .ok_or_else(|| Error::invalid("document revision overflow"))?;
        let mut attached = BTreeMap::new();
        for blob in commit.blobs {
            let bytes = blob.bytes();
            if bytes.len() > self.options.max_blob_bytes {
                return Err(Error::invalid(format!(
                    "blob {} exceeds the configured size limit",
                    blob.id()
                )));
            }
            attached.insert(blob.id(), bytes);
        }
        let existing_references = commit
            .references
            .iter()
            .filter(|id| !attached.contains_key(*id))
            .copied()
            .collect();
        if let Some(id) = self.engine.missing_blobs(&existing_references)?.first() {
            return Err(Error::BlobNotFound(*id));
        }
        let attached_ids = attached.keys().copied().collect();
        let missing_attached = self
            .engine
            .missing_blobs(&attached_ids)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        attached.retain(|id, _| missing_attached.contains(id));

        let stored = StoredEntry {
            label: commit.label.as_deref().map(str::to_owned),
            forward: commit.forward,
            inverse: commit.inverse,
            blobs: commit.references.iter().copied().collect(),
        };
        let encoded = revision::to_vec(&stored)?;
        validate_size(&encoded, "history entry")?;

        let mut next = self.head.clone();
        next.revision = revision;
        if checkpoint_required {
            next.checkpoint_revision = revision;
            next.commits_since_checkpoint = 0;
            next.bytes_since_checkpoint = 0;
        } else {
            next.commits_since_checkpoint = next.commits_since_checkpoint.saturating_add(1);
            next.bytes_since_checkpoint = next
                .bytes_since_checkpoint
                .saturating_add(commit_bytes as u64);
        }
        self.engine.commit(
            &self.head,
            &next,
            revision,
            &encoded,
            &attached,
            checkpoint.filter(|_| checkpoint_required).as_deref(),
        )?;
        self.head = next;
        Ok(revision)
    }

    pub fn changes(&self) -> Result<Refresh> {
        let head = self.engine.head()?;
        if head.document != self.head.document || head.revision < self.head.revision {
            return Err(Error::NotAProject);
        }
        Ok(Refresh {
            from: self.head.revision,
            to: head.revision,
            entries: decode_entries(
                self.engine
                    .history_range(self.head.revision, head.revision)?,
            )?,
            checkpoint_revision: head.checkpoint_revision,
            commits_since_checkpoint: head.commits_since_checkpoint,
            bytes_since_checkpoint: head.bytes_since_checkpoint,
        })
    }

    pub fn accept(&mut self, refresh: &Refresh) -> Result<()> {
        if refresh.from != self.head.revision
            || refresh
                .entries
                .last()
                .map_or(refresh.from, |entry| entry.revision)
                != refresh.to
        {
            return Err(Error::RevisionConflict {
                expected: refresh.from,
                actual: self.head.revision,
            });
        }
        self.head.revision = refresh.to;
        self.head.checkpoint_revision = refresh.checkpoint_revision;
        self.head.commits_since_checkpoint = refresh.commits_since_checkpoint;
        self.head.bytes_since_checkpoint = refresh.bytes_since_checkpoint;
        Ok(())
    }

    pub fn history(&self, revision: Revision) -> Result<HistoryEntry> {
        let encoded = self
            .engine
            .history(revision)?
            .ok_or(Error::HistoryNotFound(revision))?;
        decode_entry(revision, &encoded)
    }

    #[tracing::instrument(
        skip_all,
        fields(revision = %self.head.revision, checkpoint_bytes = checkpoint.len())
    )]
    pub fn checkpoint(&mut self, checkpoint: Vec<u8>) -> Result<()> {
        validate_size(&checkpoint, "checkpoint")?;
        let mut next = self.head.clone();
        next.checkpoint_revision = self.head.revision;
        next.commits_since_checkpoint = 0;
        next.bytes_since_checkpoint = 0;
        self.engine.checkpoint(&self.head, &next, &checkpoint)?;
        self.head = next;
        self.engine.flush()
    }

    pub fn prune_history(
        &mut self,
        keep_from: Revision,
        checkpoint: Vec<u8>,
        referenced: BTreeSet<BlobId>,
    ) -> Result<GcReport> {
        if keep_from > self.head.revision.next().unwrap_or(self.head.revision) {
            return Err(Error::invalid("history retention begins after the head"));
        }
        validate_size(&checkpoint, "checkpoint")?;
        let mut next = self.head.clone();
        next.checkpoint_revision = self.head.revision;
        next.commits_since_checkpoint = 0;
        next.bytes_since_checkpoint = 0;
        let (report, _) = self.engine.collect(
            &self.head,
            Some((&next, &checkpoint, keep_from)),
            referenced,
        )?;
        self.head = next;
        self.engine.flush()?;
        Ok(report)
    }

    #[tracing::instrument(
        skip_all,
        fields(revision = %self.head.revision, referenced_blobs = referenced.len())
    )]
    pub fn gc(&mut self, referenced: BTreeSet<BlobId>) -> Result<GcReport> {
        let (report, _) = self.engine.collect(&self.head, None, referenced)?;
        self.engine.flush()?;
        Ok(report)
    }

    pub fn compact(
        &mut self,
        keep_from: Revision,
        checkpoint: Vec<u8>,
        referenced: BTreeSet<BlobId>,
    ) -> Result<GcReport> {
        let report = self.prune_history(keep_from, checkpoint, referenced)?;
        self.engine.compact()?;
        Ok(report)
    }

    #[tracing::instrument(skip_all, fields(revision = %self.head.revision))]
    pub fn flush(&self) -> Result<()> {
        self.engine.flush()
    }
}

fn decode_entries(entries: Vec<(Revision, Vec<u8>)>) -> Result<Vec<HistoryEntry>> {
    entries
        .into_iter()
        .map(|(revision, encoded)| decode_entry(revision, &encoded))
        .collect()
}

fn decode_entry(revision: Revision, encoded: &[u8]) -> Result<HistoryEntry> {
    validate_size(encoded, "history entry")?;
    let stored: StoredEntry = revision::from_slice(encoded)?;
    Ok(stored.into_history(revision))
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

fn validate_size(bytes: &[u8], name: &str) -> Result<()> {
    if bytes.len() > MAX_RECORD_BYTES {
        Err(Error::invalid(format!("{name} exceeds the storage limit")))
    } else {
        Ok(())
    }
}

fn reached(value: u64, threshold: u64) -> bool {
    threshold != 0 && value >= threshold
}
