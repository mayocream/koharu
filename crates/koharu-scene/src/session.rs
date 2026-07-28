use std::path::Path;

use crate::{
    Children, ProjectId, Result, SceneChangeSet, ScenePatch, SceneSnapshot, ValidationContext,
    component::{encode, key},
};

pub struct SceneSession {
    storage: koharu_storage::Session,
    current: SceneSnapshot,
}

impl std::fmt::Debug for SceneSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SceneSession")
            .field("project", &self.project_id())
            .field("revision", &self.storage.revision())
            .finish_non_exhaustive()
    }
}

impl SceneSession {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let mut storage = koharu_storage::Session::create(path)?;
        initialize(&mut storage)?;
        Self::assemble(storage)
    }

    pub fn create_with(path: impl AsRef<Path>, options: koharu_storage::Options) -> Result<Self> {
        let mut storage = koharu_storage::Session::create_with(path, options)?;
        initialize(&mut storage)?;
        Self::assemble(storage)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let storage = koharu_storage::Session::open(path)?;
        Self::assemble(storage)
    }

    pub fn open_with(path: impl AsRef<Path>, options: koharu_storage::Options) -> Result<Self> {
        let storage = koharu_storage::Session::open_with(path, options)?;
        Self::assemble(storage)
    }

    pub fn memory() -> Result<Self> {
        let mut storage = koharu_storage::Session::memory()?;
        initialize(&mut storage)?;
        Self::assemble(storage)
    }

    pub fn memory_with(options: koharu_storage::Options) -> Result<Self> {
        let mut storage = koharu_storage::Session::memory_with(options)?;
        initialize(&mut storage)?;
        Self::assemble(storage)
    }

    #[must_use]
    pub fn project_id(&self) -> ProjectId {
        ProjectId(self.storage.document_id())
    }

    pub fn snapshot(&self) -> SceneSnapshot {
        self.current.clone()
    }

    pub fn commit(&mut self, patch: ScenePatch) -> Result<SceneCommit> {
        let before = self.snapshot();
        let preview = before.preview([&patch])?;
        let committed = self.storage.commit(patch.storage)?;
        let snapshot = SceneSnapshot::from_parts(committed.snapshot, preview.index);
        let changes = SceneChangeSet::from_storage(&committed.changes, &before, &snapshot);
        self.current = snapshot.clone();
        Ok(SceneCommit {
            revision: committed.revision,
            changes,
            snapshot,
        })
    }

    pub fn refresh(&mut self) -> Result<SceneChangeSet> {
        let before = self.snapshot();
        let changes = self.storage.refresh()?;
        let after = if changes.from == changes.to {
            before.clone()
        } else {
            SceneSnapshot::from_storage(self.storage.snapshot())?
        };
        self.current = after.clone();
        Ok(SceneChangeSet::from_storage(&changes, &before, &after))
    }

    pub fn undo(&mut self, revision: crate::Revision) -> Result<SceneCommit> {
        self.undo_many([revision])
    }

    pub fn undo_many(
        &mut self,
        revisions: impl IntoIterator<Item = crate::Revision>,
    ) -> Result<SceneCommit> {
        let before = self.snapshot();
        let committed = self.storage.undo_many(revisions)?;
        let snapshot = SceneSnapshot::from_storage(committed.snapshot)?;
        let changes = SceneChangeSet::from_storage(&committed.changes, &before, &snapshot);
        self.current = snapshot.clone();
        Ok(SceneCommit {
            revision: committed.revision,
            changes,
            snapshot,
        })
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        self.storage.checkpoint().map_err(Into::into)
    }

    pub fn prune_history(
        &mut self,
        keep_from: crate::Revision,
    ) -> Result<koharu_storage::GcReport> {
        self.storage.prune_history(keep_from).map_err(Into::into)
    }

    pub fn gc(&mut self) -> Result<koharu_storage::GcReport> {
        self.storage.gc().map_err(Into::into)
    }

    pub fn backup(&self, path: impl AsRef<Path>) -> Result<()> {
        self.storage.backup(path).map_err(Into::into)
    }

    fn assemble(storage: koharu_storage::Session) -> Result<Self> {
        let current = SceneSnapshot::from_storage(storage.snapshot())?;
        Ok(Self { storage, current })
    }
}

#[derive(Clone, Debug)]
pub struct SceneCommit {
    pub revision: crate::Revision,
    pub changes: SceneChangeSet,
    pub snapshot: SceneSnapshot,
}

fn initialize(storage: &mut koharu_storage::Session) -> Result<()> {
    let snapshot = storage.snapshot();
    let mut edit = snapshot.edit();
    let record_exists = |id: crate::EntityId| snapshot.contains_record(id.storage());
    let blob_exists = |_id| false;
    let children = encode(
        &Children::default(),
        &ValidationContext::new(&record_exists, &blob_exists),
    )?;
    edit.set_component(snapshot.root(), key::<Children>("default")?, children)?;
    storage.commit(edit.finish()?.with_label("Initialize scene"))?;
    Ok(())
}
