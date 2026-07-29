use std::sync::Arc;

use crate::{
    Asset, AssetInput, AssetRole, Children, ComponentSlot, EntityId, EntityOrigin, Error,
    Generation, LanguageTag, Origin, Page, PageDraft, Relation, RelationId, RelationKind, Result,
    SceneComponent, ScenePatch, SceneSnapshot, SourceText, Translation, ValidationContext,
    component::{decode, encode, key},
    index::Parent,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum At {
    Start,
    End,
    Before(EntityId),
    After(EntityId),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RemovePolicy {
    RejectNonEmpty,
    Cascade,
    PromoteChildren,
}

pub struct SceneEdit {
    base: SceneSnapshot,
    storage: koharu_storage::Edit,
    index: crate::index::SceneIndex,
    generation: Option<Generation>,
}

impl SceneEdit {
    pub(crate) fn new(base: SceneSnapshot, generation: Option<Generation>) -> Self {
        Self {
            storage: base.storage.edit(),
            index: (*base.index).clone(),
            base,
            generation,
        }
    }

    /// Marks a page or entity subtree, plus its incident relations, as input
    /// to this edit. An explicit rebase is rejected if any observed record
    /// changed after the edit's base snapshot.
    pub fn observe_subtree(&mut self, root: EntityId) -> Result<()> {
        if !self.index.parents.contains_key(&root) {
            return Err(Error::EntityNotFound(root));
        }
        let entities = self.index.descendants(root);
        for entity in &entities {
            self.storage.observe_record(entity.storage())?;
        }
        for (id, relation) in &self.index.relations {
            if entities.contains(&relation.source) || entities.contains(&relation.target) {
                self.storage.observe_record(id.storage())?;
            }
        }
        Ok(())
    }

    /// Marks one typed component, including its absence, as input to this
    /// edit without changing it.
    pub fn observe<T: SceneComponent>(
        &mut self,
        entity: EntityId,
        slot: impl Into<ComponentSlot>,
    ) -> Result<()> {
        if !self.index.parents.contains_key(&entity) {
            return Err(Error::EntityNotFound(entity));
        }
        self.storage
            .observe_component(entity.storage(), &key::<T>(slot.into())?)?;
        Ok(())
    }

    pub fn add_page(&mut self, page: PageDraft, at: At) -> Result<EntityId> {
        let id = EntityId::from_storage(self.storage.insert_record()?);
        self.set_record(id.storage(), "default", &Page::from(page))?;
        self.set_record(
            id.storage(),
            "default",
            &EntityOrigin {
                origin: self.lifecycle_origin(),
            },
        )?;
        let root = self.base.storage.root();
        let mut pages = self.read_children(root)?;
        insert_at(pages.as_mut_vec(), id, at)?;
        self.write_children(root, &pages)?;
        self.index.pages = Arc::from(pages.as_slice());
        self.index.parents.insert(id, Parent::Project);
        Ok(id)
    }

    pub fn add_entity(&mut self, parent: EntityId, at: At) -> Result<EntityId> {
        if !self.index.parents.contains_key(&parent) {
            return Err(Error::EntityNotFound(parent));
        }
        let id = EntityId::from_storage(self.storage.insert_record()?);
        self.set_record(
            id.storage(),
            "default",
            &EntityOrigin {
                origin: self.lifecycle_origin(),
            },
        )?;
        let mut children = self.read_children(parent.storage())?;
        insert_at(children.as_mut_vec(), id, at)?;
        self.write_children(parent.storage(), &children)?;
        self.index
            .children
            .insert(parent, Arc::from(children.as_slice()));
        self.index.parents.insert(id, Parent::Entity(parent));
        Ok(id)
    }

    /// Replaces user-editable page metadata without changing page identity or children.
    pub fn set_page(&mut self, entity: EntityId, page: PageDraft) -> Result<()> {
        if !self.index.pages.contains(&entity) {
            return Err(Error::EntityNotFound(entity));
        }
        self.set_record(entity.storage(), "default", &Page::from(page))
    }

    pub fn move_entity(
        &mut self,
        entity: EntityId,
        parent: Option<EntityId>,
        at: At,
    ) -> Result<()> {
        let old_parent = self
            .index
            .parents
            .get(&entity)
            .copied()
            .ok_or(Error::EntityNotFound(entity))?;
        if parent.is_none() && self.get::<Page>(entity.storage(), "default")?.is_none() {
            return Err(Error::invalid(
                "only pages may be moved to the project root",
            ));
        }
        if parent.is_some() && self.get::<Page>(entity.storage(), "default")?.is_some() {
            return Err(Error::invalid("pages must remain under the project root"));
        }
        if let Some(parent) = parent {
            if !self.index.parents.contains_key(&parent) {
                return Err(Error::EntityNotFound(parent));
            }
            if self.index.descendants(entity).contains(&parent) {
                return Err(Error::HierarchyCycle);
            }
        }
        let old_record = match old_parent {
            Parent::Project => self.base.storage.root(),
            Parent::Entity(id) => id.storage(),
        };
        let mut old_children = self.read_children(old_record)?;
        old_children.as_mut_vec().retain(|id| *id != entity);
        self.write_children(old_record, &old_children)?;
        match old_parent {
            Parent::Project => self.index.pages = Arc::from(old_children.as_slice()),
            Parent::Entity(id) => {
                self.index
                    .children
                    .insert(id, Arc::from(old_children.as_slice()));
            }
        }

        let new_record = parent.map_or(self.base.storage.root(), EntityId::storage);
        let mut new_children = self.read_children(new_record)?;
        insert_at(new_children.as_mut_vec(), entity, at)?;
        self.write_children(new_record, &new_children)?;
        if let Some(parent) = parent {
            self.index
                .children
                .insert(parent, Arc::from(new_children.as_slice()));
            self.index.parents.insert(entity, Parent::Entity(parent));
        } else {
            self.index.pages = Arc::from(new_children.as_slice());
            self.index.parents.insert(entity, Parent::Project);
        }
        Ok(())
    }

    pub fn remove_entity(&mut self, entity: EntityId, policy: RemovePolicy) -> Result<()> {
        if !self.index.parents.contains_key(&entity) {
            return Err(Error::EntityNotFound(entity));
        }
        let subtree = self.index.descendants(entity);
        for id in &subtree {
            self.validate_lifecycle_removal(*id)?;
        }
        let children = self
            .index
            .children
            .get(&entity)
            .map(|children| children.to_vec())
            .unwrap_or_default();
        if policy == RemovePolicy::PromoteChildren
            && !children.is_empty()
            && self.get::<Page>(entity.storage(), "default")?.is_some()
        {
            return Err(Error::invalid(
                "page children cannot be promoted to the project root",
            ));
        }
        if policy == RemovePolicy::RejectNonEmpty && !children.is_empty() {
            return Err(Error::NonEmptyEntity(entity));
        }
        let incident = self.incident_relations(entity);
        if policy != RemovePolicy::Cascade && !incident.is_empty() {
            return Err(Error::IncidentRelations(entity));
        }
        match policy {
            RemovePolicy::RejectNonEmpty => self.remove_single(entity, &[])?,
            RemovePolicy::PromoteChildren => self.remove_single(entity, &children)?,
            RemovePolicy::Cascade => {
                let relation_ids = subtree
                    .iter()
                    .flat_map(|id| self.incident_relations(*id))
                    .collect::<std::collections::BTreeSet<_>>();
                for relation in relation_ids {
                    self.remove_relation(relation)?;
                }
                self.detach_from_parent(entity, &[])?;
                // Clearing every component first removes all hierarchy and
                // relation references before any record lifecycle operation.
                let mut ids = subtree.iter().copied().collect::<Vec<_>>();
                for id in &ids {
                    self.clear_record(id.storage())?;
                }
                ids.reverse();
                for id in ids {
                    self.storage.remove_record(id.storage())?;
                    self.index.parents.remove(&id);
                    self.index.children.remove(&id);
                }
            }
        }
        Ok(())
    }

    pub fn promote_entity_to_user(&mut self, entity: EntityId) -> Result<()> {
        if self.generation.is_some() {
            return Err(Error::Authorship(
                "pipeline edits cannot claim user entity ownership".to_owned(),
            ));
        }
        if !self.index.parents.contains_key(&entity) {
            return Err(Error::EntityNotFound(entity));
        }
        self.set_record(
            entity.storage(),
            "default",
            &EntityOrigin {
                origin: Origin::User,
            },
        )
    }

    pub fn set<T: SceneComponent>(
        &mut self,
        entity: EntityId,
        slot: impl Into<ComponentSlot>,
        value: &T,
    ) -> Result<()> {
        let slot = slot.into();
        if !self.index.parents.contains_key(&entity) {
            return Err(Error::EntityNotFound(entity));
        }
        if matches!(
            T::KIND,
            Children::KIND | Relation::KIND | Page::KIND | EntityOrigin::KIND
        ) {
            return Err(Error::invalid(
                "structural components use dedicated scene methods",
            ));
        }
        let value = self.prepare_value(entity.storage(), slot.clone(), value)?;
        self.set_record(entity.storage(), slot, &value)
    }

    pub fn set_project<T: SceneComponent>(
        &mut self,
        slot: impl Into<ComponentSlot>,
        value: &T,
    ) -> Result<()> {
        if matches!(
            T::KIND,
            Children::KIND | Relation::KIND | Page::KIND | EntityOrigin::KIND
        ) {
            return Err(Error::invalid(
                "project structural components use dedicated scene methods",
            ));
        }
        let slot = slot.into();
        let root = self.base.storage.root();
        let value = self.prepare_value(root, slot.clone(), value)?;
        self.set_record(root, slot, &value)
    }

    pub fn remove_project<T: SceneComponent>(
        &mut self,
        slot: impl Into<ComponentSlot>,
    ) -> Result<()> {
        if matches!(
            T::KIND,
            Children::KIND | Relation::KIND | Page::KIND | EntityOrigin::KIND
        ) {
            return Err(Error::invalid(
                "project structural components cannot be removed generically",
            ));
        }
        let slot = slot.into();
        let root = self.base.storage.root();
        self.validate_removal_authorship::<T>(root, slot.clone())?;
        self.storage.remove_component(root, &key::<T>(slot)?)?;
        Ok(())
    }

    pub fn remove<T: SceneComponent>(
        &mut self,
        entity: EntityId,
        slot: impl Into<ComponentSlot>,
    ) -> Result<()> {
        if !self.index.parents.contains_key(&entity) {
            return Err(Error::EntityNotFound(entity));
        }
        if matches!(
            T::KIND,
            Children::KIND | Relation::KIND | Page::KIND | EntityOrigin::KIND
        ) {
            return Err(Error::invalid(
                "required structural components cannot be removed generically",
            ));
        }
        let slot = slot.into();
        self.validate_removal_authorship::<T>(entity.storage(), slot.clone())?;
        let key = key::<T>(slot)?;
        self.storage.remove_component(entity.storage(), &key)?;
        Ok(())
    }

    pub fn set_translation(
        &mut self,
        entity: EntityId,
        locale: &LanguageTag,
        value: Translation,
    ) -> Result<()> {
        self.set(entity, locale.as_str(), &value)
    }

    pub fn set_source_text(&mut self, entity: EntityId, value: SourceText) -> Result<()> {
        self.set(entity, "default", &value)
    }

    pub fn set_asset(
        &mut self,
        entity: EntityId,
        role: &AssetRole,
        value: AssetInput,
    ) -> Result<()> {
        let blob = self.storage.attach_blob(value.bytes);
        self.set(
            entity,
            role.as_str(),
            &Asset {
                origin: Origin::User,
                blob,
                media_type: value.media_type,
                metadata: value.metadata,
            },
        )
    }

    pub fn add_relation(
        &mut self,
        kind: RelationKind,
        source: EntityId,
        target: EntityId,
    ) -> Result<RelationId> {
        if !self.index.parents.contains_key(&source) {
            return Err(Error::EntityNotFound(source));
        }
        if !self.index.parents.contains_key(&target) {
            return Err(Error::EntityNotFound(target));
        }
        let id = RelationId::from_storage(self.storage.insert_record()?);
        let relation = Relation {
            origin: self.lifecycle_origin(),
            kind,
            source,
            target,
        };
        self.set_record(id.storage(), "default", &relation)?;
        self.index.relations.insert(id, relation.clone());
        append_relation(&mut self.index.outgoing, source, id);
        append_relation(&mut self.index.incoming, target, id);
        Ok(id)
    }

    pub fn remove_relation(&mut self, id: RelationId) -> Result<()> {
        let relation = self
            .index
            .relations
            .get(&id)
            .cloned()
            .ok_or(Error::RelationNotFound(id))?;
        self.validate_origin_removal(Some(&relation.origin))?;
        self.index.relations.remove(&id);
        self.clear_record(id.storage())?;
        self.storage.remove_record(id.storage())?;
        remove_relation_id(&mut self.index.outgoing, relation.source, id);
        remove_relation_id(&mut self.index.incoming, relation.target, id);
        Ok(())
    }

    pub fn promote_relation_to_user(&mut self, id: RelationId) -> Result<()> {
        if self.generation.is_some() {
            return Err(Error::Authorship(
                "pipeline edits cannot claim user relation ownership".to_owned(),
            ));
        }
        let relation = self
            .index
            .relations
            .get(&id)
            .cloned()
            .ok_or(Error::RelationNotFound(id))?;
        let promoted = Relation {
            origin: Origin::User,
            ..relation
        };
        self.set_record(id.storage(), "default", &promoted)?;
        self.index.relations.insert(id, promoted);
        Ok(())
    }

    pub fn set_relation<T: SceneComponent>(
        &mut self,
        relation: RelationId,
        slot: impl Into<ComponentSlot>,
        value: &T,
    ) -> Result<()> {
        if !self.index.relations.contains_key(&relation) {
            return Err(Error::RelationNotFound(relation));
        }
        if matches!(
            T::KIND,
            Children::KIND | Relation::KIND | Page::KIND | EntityOrigin::KIND
        ) {
            return Err(Error::invalid(
                "relation structural components use dedicated scene methods",
            ));
        }
        let slot = slot.into();
        let value = self.prepare_value(relation.storage(), slot.clone(), value)?;
        self.set_record(relation.storage(), slot, &value)
    }

    pub fn remove_relation_component<T: SceneComponent>(
        &mut self,
        relation: RelationId,
        slot: impl Into<ComponentSlot>,
    ) -> Result<()> {
        if !self.index.relations.contains_key(&relation) {
            return Err(Error::RelationNotFound(relation));
        }
        if matches!(
            T::KIND,
            Children::KIND | Relation::KIND | Page::KIND | EntityOrigin::KIND
        ) {
            return Err(Error::invalid(
                "relation structural components cannot be removed generically",
            ));
        }
        let slot = slot.into();
        self.validate_removal_authorship::<T>(relation.storage(), slot.clone())?;
        self.storage
            .remove_component(relation.storage(), &key::<T>(slot)?)?;
        Ok(())
    }

    pub fn finish(self) -> Result<ScenePatch> {
        let mut patch = ScenePatch {
            storage: self.storage.finish()?,
            result_index: None,
        };
        let preview = self.base.preview([&patch])?;
        patch.result_index = Some(preview.index);
        Ok(patch)
    }

    fn set_record<T: SceneComponent>(
        &mut self,
        record: koharu_storage::RecordId,
        slot: impl Into<ComponentSlot>,
        value: &T,
    ) -> Result<()> {
        let slot = slot.into();
        let record_exists = |id: EntityId| self.storage.view().contains_record(id.storage());
        // Attached blobs are verified by storage preview. Structural component
        // validation must not force a byte read.
        let blob_exists = |_id| true;
        let context = ValidationContext::new(&record_exists, &blob_exists);
        let value = encode(value, &context)?;
        self.storage.set_component(record, key::<T>(slot)?, value)?;
        Ok(())
    }

    fn prepare_value<T: SceneComponent>(
        &mut self,
        record: koharu_storage::RecordId,
        slot: ComponentSlot,
        value: &T,
    ) -> Result<T> {
        self.validate_removal_authorship::<T>(record, slot)?;
        let mut value = value.clone();
        match &self.generation {
            Some(generation) => {
                if !value.set_origin(Origin::Generated(generation.clone())) {
                    return Err(Error::Authorship(format!(
                        "pipeline cannot write unmanaged component {}",
                        T::KIND
                    )));
                }
            }
            None if value.origin().is_some() && !value.set_origin(Origin::User) => {
                return Err(Error::Authorship(format!(
                    "component {} reports ownership but cannot be stamped",
                    T::KIND
                )));
            }
            None => {}
        }
        Ok(value)
    }

    fn validate_removal_authorship<T: SceneComponent>(
        &mut self,
        record: koharu_storage::RecordId,
        slot: ComponentSlot,
    ) -> Result<()> {
        let existing = self.get::<T>(record, slot.as_str())?;
        match existing {
            None => Ok(()),
            Some(value) => {
                if self.generation.is_some() && value.origin().is_none() {
                    return Err(Error::Authorship(format!(
                        "pipeline cannot remove unmanaged component {}",
                        T::KIND
                    )));
                }
                self.validate_origin_removal(value.origin())
            }
        }
    }

    fn validate_origin_removal(&self, existing: Option<&Origin>) -> Result<()> {
        let Some(expected) = &self.generation else {
            return Ok(());
        };
        let Some(existing) = existing else {
            return Ok(());
        };
        match existing {
            Origin::User => Err(Error::Authorship(
                "pipeline cannot overwrite or remove a user-owned component".to_owned(),
            )),
            Origin::Generated(actual) if expected.producer != actual.producer => {
                Err(Error::Authorship(format!(
                    "producer {} owns this component, not {}",
                    actual.producer, expected.producer
                )))
            }
            Origin::Generated(_) => Ok(()),
        }
    }

    fn lifecycle_origin(&self) -> Origin {
        self.generation
            .clone()
            .map_or(Origin::User, Origin::Generated)
    }

    fn validate_lifecycle_removal(&mut self, entity: EntityId) -> Result<()> {
        let Some(producer) = self
            .generation
            .as_ref()
            .map(|generation| generation.producer.clone())
        else {
            return Ok(());
        };
        let lifecycle = self
            .get::<EntityOrigin>(entity.storage(), "default")?
            .ok_or_else(|| Error::invalid("entity lifecycle origin is missing"))?;
        match lifecycle.origin {
            Origin::Generated(owner) if owner.producer == producer => Ok(()),
            Origin::Generated(owner) => Err(Error::Authorship(format!(
                "producer {} owns entity {entity}, not {}",
                owner.producer, producer
            ))),
            Origin::User => Err(Error::Authorship(format!(
                "pipeline cannot remove user-owned entity {entity}"
            ))),
        }
    }

    fn get<T: SceneComponent>(
        &mut self,
        record: koharu_storage::RecordId,
        slot: &str,
    ) -> Result<Option<T>> {
        let key = key::<T>(slot)?;
        self.storage.observe_component(record, &key)?;
        let view = self.storage.view();
        let Some(raw) = view.component(record, &key)? else {
            return Ok(None);
        };
        let record_exists = |id: EntityId| view.contains_record(id.storage());
        let blob_exists = |_id| true;
        decode(
            slot,
            raw,
            &ValidationContext::new(&record_exists, &blob_exists),
        )
        .map(Some)
    }

    fn read_children(&mut self, record: koharu_storage::RecordId) -> Result<Children> {
        Ok(self.get::<Children>(record, "default")?.unwrap_or_default())
    }

    fn write_children(
        &mut self,
        record: koharu_storage::RecordId,
        children: &Children,
    ) -> Result<()> {
        self.set_record(record, "default", children)
    }

    fn incident_relations(&self, entity: EntityId) -> Vec<RelationId> {
        self.index
            .outgoing
            .get(&entity)
            .into_iter()
            .chain(self.index.incoming.get(&entity))
            .flat_map(|ids| ids.iter().copied())
            .collect()
    }

    fn remove_single(&mut self, entity: EntityId, promote: &[EntityId]) -> Result<()> {
        self.detach_from_parent(entity, promote)?;
        if !promote.is_empty() {
            let parent = self.index.parents[&entity];
            for child in promote {
                self.index.parents.insert(*child, parent);
            }
        }
        self.clear_record(entity.storage())?;
        self.storage.remove_record(entity.storage())?;
        self.index.parents.remove(&entity);
        self.index.children.remove(&entity);
        Ok(())
    }

    fn detach_from_parent(&mut self, entity: EntityId, promote: &[EntityId]) -> Result<()> {
        let parent = self.index.parents[&entity];
        let record = match parent {
            Parent::Project => self.base.storage.root(),
            Parent::Entity(id) => id.storage(),
        };
        let mut siblings = self.read_children(record)?;
        let position = siblings
            .as_slice()
            .iter()
            .position(|id| *id == entity)
            .ok_or_else(|| Error::invalid("parent index differs from children component"))?;
        siblings
            .as_mut_vec()
            .splice(position..=position, promote.iter().copied());
        self.write_children(record, &siblings)?;
        match parent {
            Parent::Project => self.index.pages = Arc::from(siblings.as_slice()),
            Parent::Entity(id) => {
                self.index
                    .children
                    .insert(id, Arc::from(siblings.as_slice()));
            }
        }
        Ok(())
    }

    fn clear_record(&mut self, record: koharu_storage::RecordId) -> Result<()> {
        let keys = self
            .storage
            .view()
            .record(record)?
            .components()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            self.storage.remove_component(record, &key)?;
        }
        Ok(())
    }
}

fn insert_at(values: &mut Vec<EntityId>, value: EntityId, at: At) -> Result<()> {
    if values.contains(&value) {
        return Err(Error::MultipleParents(value));
    }
    let position = match at {
        At::Start => 0,
        At::End => values.len(),
        At::Before(anchor) => values
            .iter()
            .position(|id| *id == anchor)
            .ok_or_else(|| Error::invalid("before anchor is not a sibling"))?,
        At::After(anchor) => values
            .iter()
            .position(|id| *id == anchor)
            .map(|position| position + 1)
            .ok_or_else(|| Error::invalid("after anchor is not a sibling"))?,
    };
    values.insert(position, value);
    Ok(())
}

fn append_relation(
    map: &mut imbl::OrdMap<EntityId, Arc<[RelationId]>>,
    entity: EntityId,
    relation: RelationId,
) {
    let mut values = map
        .get(&entity)
        .map(|values| values.to_vec())
        .unwrap_or_default();
    values.push(relation);
    values.sort_unstable();
    map.insert(entity, values.into());
}

fn remove_relation_id(
    map: &mut imbl::OrdMap<EntityId, Arc<[RelationId]>>,
    entity: EntityId,
    relation: RelationId,
) {
    let Some(existing) = map.get(&entity) else {
        return;
    };
    let values = existing
        .iter()
        .copied()
        .filter(|id| *id != relation)
        .collect::<Vec<_>>();
    if values.is_empty() {
        map.remove(&entity);
    } else {
        map.insert(entity, values.into());
    }
}
