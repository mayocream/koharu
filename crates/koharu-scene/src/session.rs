use std::{collections::BTreeSet, path::Path, sync::Arc};

use crate::{
    Change, Error, Patch, ProjectId, Result, Snapshot,
    patch::{apply_operations, decode_operations},
    state::{State, StoredState},
};

pub struct Session {
    storage: koharu_storage::Session,
    current: Snapshot,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Session")
            .field("project", &self.project_id())
            .field("revision", &self.storage.revision())
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::create_with(path, koharu_storage::Options::default())
    }

    pub fn create_with(path: impl AsRef<Path>, options: koharu_storage::Options) -> Result<Self> {
        let document = koharu_storage::DocumentId::new();
        let state = State::empty(document);
        let checkpoint = encode_checkpoint(&state)?;
        let storage = koharu_storage::Session::create_with(path, document, checkpoint, options)?;
        Self::assemble_new(storage, state)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, koharu_storage::Options::default())
    }

    pub fn open_with(path: impl AsRef<Path>, options: koharu_storage::Options) -> Result<Self> {
        let storage = koharu_storage::Session::open_with(path, options)?;
        Self::recover(storage)
    }

    pub fn memory() -> Result<Self> {
        Self::memory_with(koharu_storage::Options::default())
    }

    pub fn memory_with(options: koharu_storage::Options) -> Result<Self> {
        let document = koharu_storage::DocumentId::new();
        let state = State::empty(document);
        let checkpoint = encode_checkpoint(&state)?;
        let storage = koharu_storage::Session::memory_with(document, checkpoint, options)?;
        Self::assemble_new(storage, state)
    }

    #[must_use]
    pub fn project_id(&self) -> ProjectId {
        ProjectId(self.storage.document_id())
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        self.current.clone()
    }

    pub fn commit(&mut self, patch: Patch) -> Result<Commit> {
        if patch.project != self.storage.document_id() {
            return Err(Error::invalid("patch belongs to another project"));
        }
        if patch.base_revision != self.storage.revision()
            || !Arc::ptr_eq(&patch.base_state, &self.current.state)
        {
            return Err(Error::Storage(koharu_storage::Error::RevisionConflict {
                expected: patch.base_revision,
                actual: self.storage.revision(),
            }));
        }
        if patch.is_empty() {
            return Ok(Commit {
                revision: self.current.revision(),
                changes: Change::empty(self.current.revision()),
                snapshot: self.current.clone(),
            });
        }

        let forward = patch.forward_bytes()?;
        let inverse = patch.inverse_bytes()?;
        let next_revision = self
            .storage
            .revision()
            .next()
            .ok_or_else(|| Error::invalid("project revision overflow"))?;
        let mut state = (*patch.state).clone();
        state.revision = next_revision;
        let referenced = state.referenced_blobs();
        self.current.blobs.preview(
            next_revision,
            patch.attachments.iter().cloned(),
            referenced.clone(),
        )?;

        let mut request = koharu_storage::CommitRequest::new(
            self.storage.document_id(),
            self.storage.revision(),
            forward,
            inverse,
        )
        .with_label(patch.label.clone());
        request.reference_blobs(patch.referenced_blobs());
        for attachment in patch.attachments.iter().cloned() {
            request.attach(attachment);
        }
        let checkpoint = self
            .storage
            .checkpoint_due(request.forward.len() + request.inverse.len())?
            .then(|| encode_checkpoint(&state))
            .transpose()?;
        let committed = self.storage.commit(request, checkpoint, referenced)?;
        let state = Arc::new(state);
        let snapshot = Snapshot::new(state, committed.snapshot)?;
        let changes =
            Change::from_operations(patch.base_revision, committed.revision, &patch.operations);
        self.current = snapshot.clone();
        Ok(Commit {
            revision: committed.revision,
            changes,
            snapshot,
        })
    }

    pub fn refresh(&mut self) -> Result<Change> {
        let refresh = self.storage.prepare_refresh()?;
        if refresh.from == refresh.to {
            return Ok(Change::empty(refresh.from));
        }
        let mut state = (*self.current.state).clone();
        let mut operations = Vec::new();
        for commit in &refresh.commits {
            let decoded = decode_operations(&commit.forward)?;
            state = apply_operations(&state, &decoded)?;
            state.revision = commit.revision;
            operations.extend(decoded);
        }
        self.storage.accept_refresh(&refresh)?;
        let blobs = self.storage.snapshot(state.referenced_blobs());
        let snapshot = Snapshot::new(Arc::new(state), blobs)?;
        let changes = Change::from_operations(refresh.from, refresh.to, &operations);
        self.current = snapshot;
        Ok(changes)
    }

    pub fn undo(&mut self, revision: crate::Revision) -> Result<Commit> {
        self.undo_many([revision])
    }

    pub fn undo_many(
        &mut self,
        revisions: impl IntoIterator<Item = crate::Revision>,
    ) -> Result<Commit> {
        let mut revisions = revisions.into_iter().collect::<Vec<_>>();
        revisions.sort_unstable_by(|left, right| right.cmp(left));
        revisions.dedup();
        if revisions.is_empty() {
            return Err(Error::invalid("undo requires at least one revision"));
        }
        let mut operations = Vec::new();
        for revision in &revisions {
            operations.extend(decode_operations(
                &self.storage.history(*revision)?.inverse,
            )?);
        }
        let state = apply_operations(&self.current.state, &operations)?;
        let label: Arc<str> = if revisions.len() == 1 {
            format!("Undo revision {}", revisions[0]).into()
        } else {
            format!("Undo {} revisions", revisions.len()).into()
        };
        let patch = Patch::new(
            &self.current,
            state,
            Vec::new(),
            operations,
            Vec::new(),
            Some(label),
        )?;
        self.commit(patch)
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        self.storage
            .checkpoint(encode_checkpoint(&self.current.state)?)?;
        Ok(())
    }

    pub fn prune_history(
        &mut self,
        keep_from: crate::Revision,
    ) -> Result<koharu_storage::GcReport> {
        self.storage
            .prune_history(
                keep_from,
                encode_checkpoint(&self.current.state)?,
                self.current.state.referenced_blobs(),
            )
            .map_err(Into::into)
    }

    pub fn gc(&mut self) -> Result<koharu_storage::GcReport> {
        self.storage
            .gc(self.current.state.referenced_blobs())
            .map_err(Into::into)
    }

    pub fn backup(&self, path: impl AsRef<Path>) -> Result<()> {
        self.storage.backup(path).map_err(Into::into)
    }

    pub fn flush(&self) -> Result<()> {
        self.storage.flush().map_err(Into::into)
    }

    fn assemble_new(storage: koharu_storage::Session, state: State) -> Result<Self> {
        let blobs = storage.snapshot(BTreeSet::new());
        let current = Snapshot::new(Arc::new(state), blobs)?;
        Ok(Self { storage, current })
    }

    fn recover(storage: koharu_storage::Session) -> Result<Self> {
        let recovery = storage.recovery()?;
        let checkpoint: StoredState = revision::from_slice(&recovery.checkpoint)?;
        let mut state =
            State::from_checkpoint(recovery.document, recovery.checkpoint_revision, checkpoint)?;
        for commit in &recovery.commits {
            state = apply_operations(&state, &decode_operations(&commit.forward)?)?;
            state.revision = commit.revision;
        }
        if state.revision != recovery.head {
            return Err(Error::invalid("project history does not reach its head"));
        }
        state.validate()?;
        let blobs = storage.snapshot(state.referenced_blobs());
        let current = Snapshot::new(Arc::new(state), blobs)?;
        Ok(Self { storage, current })
    }
}

#[derive(Clone, Debug)]
pub struct Commit {
    pub revision: crate::Revision,
    pub changes: Change,
    pub snapshot: Snapshot,
}

fn encode_checkpoint(state: &State) -> Result<Vec<u8>> {
    revision::to_vec(&state.to_checkpoint()).map_err(Into::into)
}
