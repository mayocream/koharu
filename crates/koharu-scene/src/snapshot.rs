use std::sync::Arc;

use crate::{
    BlobId, ComponentSlot, EntityId, Error, ProjectId, RelationId, RelationKind, Result,
    SceneComponent, SceneEdit, ScenePatch, ValidationContext,
    component::{decode, key},
    index::{Parent, SceneIndex},
};

#[derive(Clone, Debug)]
pub struct SceneSnapshot {
    pub(crate) storage: koharu_storage::Snapshot,
    pub(crate) index: Arc<SceneIndex>,
}

impl SceneSnapshot {
    pub(crate) fn from_storage(storage: koharu_storage::Snapshot) -> Result<Self> {
        let index = Arc::new(SceneIndex::build(&storage)?);
        Ok(Self { storage, index })
    }

    pub(crate) fn from_parts(storage: koharu_storage::Snapshot, index: Arc<SceneIndex>) -> Self {
        Self { storage, index }
    }

    #[must_use]
    pub fn project_id(&self) -> ProjectId {
        ProjectId(self.storage.document_id())
    }

    #[must_use]
    pub fn revision(&self) -> crate::Revision {
        self.storage.revision()
    }

    pub fn pages(&self) -> impl ExactSizeIterator<Item = PageRef<'_>> {
        self.index
            .pages
            .iter()
            .copied()
            .map(|id| PageRef { snapshot: self, id })
    }

    pub fn entities(&self) -> impl ExactSizeIterator<Item = EntityRef<'_>> {
        self.index
            .entities
            .iter()
            .copied()
            .map(|id| EntityRef { snapshot: self, id })
    }

    pub fn subtree(&self, root: EntityId) -> Result<impl Iterator<Item = EntityRef<'_>> + '_> {
        if !self.index.parents.contains_key(&root) {
            return Err(Error::EntityNotFound(root));
        }
        let included = self.index.descendants(root);
        Ok(self
            .index
            .entities
            .iter()
            .copied()
            .filter(move |id| included.contains(id))
            .map(|id| EntityRef { snapshot: self, id }))
    }

    pub fn descendants(&self, root: EntityId) -> Result<impl Iterator<Item = EntityRef<'_>> + '_> {
        Ok(self.subtree(root)?.filter(move |entity| entity.id != root))
    }

    pub fn entities_with<T: SceneComponent>(
        &self,
        slot: impl Into<ComponentSlot>,
    ) -> Result<impl ExactSizeIterator<Item = EntityRef<'_>>> {
        let component = key::<T>(slot.into())?;
        let query = (
            component.kind().as_str().to_owned(),
            component.slot().as_str().to_owned(),
        );
        let ids = self
            .index
            .component_entities
            .get(&query)
            .map(AsRef::as_ref)
            .unwrap_or(&[]);
        Ok(ids
            .iter()
            .copied()
            .map(|id| EntityRef { snapshot: self, id }))
    }

    pub fn page(&self, id: EntityId) -> Result<PageRef<'_>> {
        if self.index.pages.contains(&id) {
            Ok(PageRef { snapshot: self, id })
        } else {
            Err(Error::EntityNotFound(id))
        }
    }

    pub fn entity(&self, id: EntityId) -> Result<EntityRef<'_>> {
        if self.index.parents.contains_key(&id) {
            Ok(EntityRef { snapshot: self, id })
        } else {
            Err(Error::EntityNotFound(id))
        }
    }

    pub fn relation(&self, id: RelationId) -> Result<RelationRef<'_>> {
        if self.index.relations.contains_key(&id) {
            Ok(RelationRef { snapshot: self, id })
        } else {
            Err(Error::RelationNotFound(id))
        }
    }

    pub fn parent(&self, id: EntityId) -> Result<Option<EntityId>> {
        match self.index.parents.get(&id).copied() {
            Some(Parent::Project) => Ok(None),
            Some(Parent::Entity(parent)) => Ok(Some(parent)),
            None => Err(Error::EntityNotFound(id)),
        }
    }

    pub fn children(&self, id: EntityId) -> Result<impl ExactSizeIterator<Item = EntityId> + '_> {
        if !self.index.parents.contains_key(&id) {
            return Err(Error::EntityNotFound(id));
        }
        Ok(self
            .index
            .children
            .get(&id)
            .map(AsRef::as_ref)
            .unwrap_or(&[])
            .iter()
            .copied())
    }

    pub fn component<T: SceneComponent>(
        &self,
        entity: EntityId,
        slot: impl Into<ComponentSlot>,
    ) -> Result<Option<T>> {
        if !self.index.parents.contains_key(&entity) {
            return Err(Error::EntityNotFound(entity));
        }
        self.component_on_record(entity.storage(), slot)
    }

    pub fn project_component<T: SceneComponent>(
        &self,
        slot: impl Into<ComponentSlot>,
    ) -> Result<Option<T>> {
        self.component_on_record(self.storage.root(), slot)
    }

    fn component_on_record<T: SceneComponent>(
        &self,
        record: koharu_storage::RecordId,
        slot: impl Into<ComponentSlot>,
    ) -> Result<Option<T>> {
        let slot = slot.into();
        let key = key::<T>(slot.clone())?;
        let Some(raw) = self.storage.component(record, &key)? else {
            return Ok(None);
        };
        let record_exists = |id: EntityId| self.storage.contains_record(id.storage());
        let blob_exists = |id| self.storage.has_blob(id);
        let context = ValidationContext::new(&record_exists, &blob_exists);
        decode::<T>(slot.as_str(), raw, &context).map(Some)
    }

    pub fn relations_from<'a>(
        &'a self,
        entity: EntityId,
        kind: Option<&'a RelationKind>,
    ) -> impl Iterator<Item = RelationRef<'a>> + 'a {
        self.index
            .outgoing
            .get(&entity)
            .into_iter()
            .flat_map(|ids| ids.iter().copied())
            .filter(move |id| kind.is_none_or(|kind| self.index.relations[id].kind == *kind))
            .map(|id| RelationRef { snapshot: self, id })
    }

    pub fn relations_to<'a>(
        &'a self,
        entity: EntityId,
        kind: Option<&'a RelationKind>,
    ) -> impl Iterator<Item = RelationRef<'a>> + 'a {
        self.index
            .incoming
            .get(&entity)
            .into_iter()
            .flat_map(|ids| ids.iter().copied())
            .filter(move |id| kind.is_none_or(|kind| self.index.relations[id].kind == *kind))
            .map(|id| RelationRef { snapshot: self, id })
    }

    pub fn read_blob(&self, id: BlobId) -> Result<Arc<[u8]>> {
        self.storage.read_blob(id).map_err(Into::into)
    }

    pub fn read_blobs(&self, ids: impl IntoIterator<Item = BlobId>) -> Result<crate::BlobBatch> {
        self.storage.read_blobs(ids).map_err(Into::into)
    }

    #[must_use]
    pub fn edit(&self) -> SceneEdit {
        SceneEdit::new(self.clone(), None)
    }

    #[must_use]
    pub fn edit_as(&self, generation: crate::Generation) -> SceneEdit {
        SceneEdit::new(self.clone(), Some(generation))
    }

    pub fn patch(&self, f: impl FnOnce(&mut SceneEdit) -> Result<()>) -> Result<ScenePatch> {
        let mut edit = self.edit();
        f(&mut edit)?;
        edit.finish()
    }

    pub fn preview<'a>(&self, patches: impl IntoIterator<Item = &'a ScenePatch>) -> Result<Self> {
        let mut current = self.clone();
        for patch in patches {
            let exact_input = patch.storage.has_exact_input(&current.storage);
            let storage = current.storage.preview([&patch.storage])?;
            let index = if exact_input {
                patch.result_index.clone().map_or_else(
                    || {
                        current
                            .index
                            .after_patch(&storage, &patch.storage.effects())
                            .map(Arc::new)
                    },
                    Ok,
                )?
            } else {
                Arc::new(
                    current
                        .index
                        .after_patch(&storage, &patch.storage.effects())?,
                )
            };
            current = Self::from_parts(storage, index);
        }
        Ok(current)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct EntityRef<'snapshot> {
    snapshot: &'snapshot SceneSnapshot,
    id: EntityId,
}

impl<'snapshot> EntityRef<'snapshot> {
    #[must_use]
    pub const fn id(self) -> EntityId {
        self.id
    }

    pub fn component<T: SceneComponent>(self, slot: impl Into<ComponentSlot>) -> Result<Option<T>> {
        self.snapshot.component(self.id, slot)
    }

    pub fn parent(self) -> Result<Option<EntityId>> {
        self.snapshot.parent(self.id)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct PageRef<'snapshot> {
    pub(crate) snapshot: &'snapshot SceneSnapshot,
    pub(crate) id: EntityId,
}

impl<'snapshot> PageRef<'snapshot> {
    #[must_use]
    pub const fn id(self) -> EntityId {
        self.id
    }

    pub fn page(self) -> Result<crate::Page> {
        self.snapshot
            .component(self.id, "default")?
            .ok_or_else(|| Error::invalid("page component is missing"))
    }

    #[must_use]
    pub fn entity(self) -> EntityRef<'snapshot> {
        EntityRef {
            snapshot: self.snapshot,
            id: self.id,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct RelationRef<'snapshot> {
    pub(crate) snapshot: &'snapshot SceneSnapshot,
    pub(crate) id: RelationId,
}

impl<'snapshot> RelationRef<'snapshot> {
    #[must_use]
    pub const fn id(self) -> RelationId {
        self.id
    }

    #[must_use]
    pub fn value(self) -> &'snapshot crate::Relation {
        &self.snapshot.index.relations[&self.id]
    }

    pub fn component<T: SceneComponent>(self, slot: impl Into<ComponentSlot>) -> Result<Option<T>> {
        self.snapshot.component_on_record(self.id.storage(), slot)
    }
}
