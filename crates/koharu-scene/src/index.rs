use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use imbl::OrdMap;

use crate::{
    Asset, Children, DetectionAnalysis, EntityId, EntityOrigin, Error, Geometry, OcrAnalysis, Page,
    ProjectSettings, ReadingOrder, Region, Relation, RelationId, Result, SceneComponent,
    SourceText, TextRole, Translation, Typography, ValidationContext, Visibility,
    component::{decode, key},
};

#[cfg(test)]
static INDEX_BUILDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const MAX_HIERARCHY_DEPTH: usize = 256;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Parent {
    Project,
    Entity(EntityId),
}

#[derive(Clone, Debug)]
pub(crate) struct SceneIndex {
    #[cfg(test)]
    build_marker: usize,
    pub(crate) pages: Arc<[EntityId]>,
    pub(crate) entities: Arc<[EntityId]>,
    pub(crate) parents: OrdMap<EntityId, Parent>,
    pub(crate) children: OrdMap<EntityId, Arc<[EntityId]>>,
    pub(crate) relations: OrdMap<RelationId, Relation>,
    pub(crate) outgoing: OrdMap<EntityId, Arc<[RelationId]>>,
    pub(crate) incoming: OrdMap<EntityId, Arc<[RelationId]>>,
    pub(crate) component_entities: OrdMap<(String, String), Arc<[EntityId]>>,
    positions: OrdMap<EntityId, usize>,
}

impl SceneIndex {
    pub(crate) fn build(snapshot: &koharu_storage::Snapshot) -> Result<Self> {
        #[cfg(test)]
        let build_marker = INDEX_BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let record_exists = |id: EntityId| snapshot.contains_record(id.storage());
        let blob_exists = |id| snapshot.has_blob(id);
        let context = ValidationContext::new(&record_exists, &blob_exists);
        let root_children = read::<Children>(snapshot, snapshot.root(), "default", &context)?
            .ok_or_else(|| Error::invalid("project root has no children component"))?;
        let relation_key = key::<Relation>("default")?;
        let page_key = key::<Page>("default")?;
        let children_key = key::<Children>("default")?;
        let origin_key = key::<EntityOrigin>("default")?;
        let root_record = snapshot.record(snapshot.root())?;
        if root_record.component(&relation_key).is_some()
            || root_record.component(&page_key).is_some()
            || root_record.component(&origin_key).is_some()
        {
            return Err(Error::invalid(
                "project root cannot be an entity or relation record",
            ));
        }
        let mut parents = BTreeMap::new();
        let mut children = BTreeMap::new();
        let mut entities = Vec::new();
        let mut stack = root_children
            .iter()
            .rev()
            .map(|id| (id, Parent::Project, 1_usize))
            .collect::<Vec<_>>();
        while let Some((id, parent, depth)) = stack.pop() {
            if depth > MAX_HIERARCHY_DEPTH {
                return Err(Error::invalid("scene hierarchy exceeds maximum depth"));
            }
            if parents.insert(id, parent).is_some() {
                return Err(Error::MultipleParents(id));
            }
            entities.push(id);
            let record = snapshot
                .record(id.storage())
                .map_err(|_| Error::EntityNotFound(id))?;
            if record.component(&relation_key).is_some() {
                return Err(Error::invalid(format!(
                    "relation record {id} appears in hierarchy"
                )));
            }
            if matches!(parent, Parent::Project) && record.component(&page_key).is_none() {
                return Err(Error::invalid(format!("root child {id} is not a page")));
            }
            if matches!(parent, Parent::Entity(_)) && record.component(&page_key).is_some() {
                return Err(Error::invalid(format!(
                    "nested entity {id} carries a page marker"
                )));
            }
            if record.component(&origin_key).is_none() {
                return Err(Error::invalid(format!(
                    "entity {id} has no lifecycle origin component"
                )));
            }
            if let Some(value) = read::<Children>(snapshot, id.storage(), "default", &context)? {
                children.insert(id, Arc::from(value.as_slice()));
                for child in value.iter().rev() {
                    stack.push((child, Parent::Entity(id), depth + 1));
                }
            }
        }

        let mut relations = BTreeMap::new();
        for record in snapshot.records() {
            if record.id() == snapshot.root() {
                continue;
            }
            let entity_id = EntityId::from_storage(record.id());
            if let Some(raw) = record.component(&relation_key) {
                if parents.contains_key(&entity_id) {
                    return Err(Error::invalid("record is both an entity and a relation"));
                }
                let relation = decode::<Relation>("default", raw, &context)?;
                if record.component(&children_key).is_some()
                    || record.component(&page_key).is_some()
                    || record.component(&origin_key).is_some()
                {
                    return Err(Error::invalid(format!(
                        "relation record {} carries an entity structural component",
                        record.id()
                    )));
                }
                let id = RelationId::from_storage(record.id());
                relations.insert(id, relation);
            } else if !parents.contains_key(&entity_id) {
                return Err(Error::invalid(format!("orphan scene record {entity_id}")));
            }
        }

        // Validate every known component without interpreting extensions.
        for record in snapshot.records() {
            for (component_key, raw) in record.components() {
                validate_known(
                    snapshot,
                    record.id(),
                    component_key.kind().as_str(),
                    component_key.slot().as_str(),
                    raw,
                    &context,
                )?;
            }
        }

        // Relations must terminate at hierarchy entities, never other relations.
        let mut outgoing = BTreeMap::<EntityId, Vec<RelationId>>::new();
        let mut incoming = BTreeMap::<EntityId, Vec<RelationId>>::new();
        for (id, relation) in &relations {
            if !parents.contains_key(&relation.source) || !parents.contains_key(&relation.target) {
                return Err(Error::invalid(format!(
                    "relation {id} endpoint is not an entity"
                )));
            }
            outgoing.entry(relation.source).or_default().push(*id);
            incoming.entry(relation.target).or_default().push(*id);
        }

        for entity in &entities {
            validate_entity_requirements(snapshot, *entity)?;
        }

        let mut component_entities = BTreeMap::<(String, String), Vec<EntityId>>::new();
        for entity in &entities {
            for (component, _) in snapshot.record(entity.storage())?.components() {
                component_entities
                    .entry((
                        component.kind().as_str().to_owned(),
                        component.slot().as_str().to_owned(),
                    ))
                    .or_default()
                    .push(*entity);
            }
        }
        let positions = entities
            .iter()
            .copied()
            .enumerate()
            .map(|(index, id)| (id, index))
            .collect();

        Ok(Self {
            #[cfg(test)]
            build_marker,
            pages: Arc::from(root_children.as_slice()),
            entities: entities.into(),
            parents: parents.into_iter().collect(),
            children: children.into_iter().collect(),
            relations: relations.into_iter().collect(),
            outgoing: outgoing
                .into_iter()
                .map(|(id, values)| (id, Arc::<[RelationId]>::from(values)))
                .collect(),
            incoming: incoming
                .into_iter()
                .map(|(id, values)| (id, Arc::<[RelationId]>::from(values)))
                .collect(),
            component_entities: component_entities
                .into_iter()
                .map(|(key, values)| (key, Arc::<[EntityId]>::from(values)))
                .collect(),
            positions,
        })
    }

    pub(crate) fn after_patch(
        &self,
        snapshot: &koharu_storage::Snapshot,
        effects: &koharu_storage::PatchEffects,
    ) -> Result<Self> {
        let structural = effects.changes_record_lifecycle()
            || effects.components().any(|address| {
                matches!(
                    address.key.kind().as_str(),
                    Children::KIND | Page::KIND | Relation::KIND | EntityOrigin::KIND
                )
            });
        if structural {
            return Self::build(snapshot);
        }

        let record_exists = |id: EntityId| snapshot.contains_record(id.storage());
        let blob_exists = |id| snapshot.has_blob(id);
        let context = ValidationContext::new(&record_exists, &blob_exists);
        let mut next = self.clone();
        let mut affected_entities = BTreeSet::new();
        for address in effects.components() {
            let raw = snapshot.component(address.record, &address.key)?;
            if let Some(raw) = raw {
                validate_known(
                    snapshot,
                    address.record,
                    address.key.kind().as_str(),
                    address.key.slot().as_str(),
                    raw,
                    &context,
                )?;
            }
            let entity = EntityId::from_storage(address.record);
            if !self.parents.contains_key(&entity) {
                continue;
            }
            affected_entities.insert(entity);
            let component_key = (
                address.key.kind().as_str().to_owned(),
                address.key.slot().as_str().to_owned(),
            );
            let mut members = next
                .component_entities
                .get(&component_key)
                .map(|members| members.to_vec())
                .unwrap_or_default();
            members.retain(|member| *member != entity);
            if raw.is_some() {
                members.push(entity);
                members.sort_by_key(|member| self.positions[member]);
            }
            if members.is_empty() {
                next.component_entities.remove(&component_key);
            } else {
                next.component_entities
                    .insert(component_key, members.into());
            }
        }
        for entity in affected_entities {
            validate_entity_requirements(snapshot, entity)?;
        }
        Ok(next)
    }

    pub(crate) fn descendants(&self, root: EntityId) -> BTreeSet<EntityId> {
        let mut result = BTreeSet::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if result.insert(id)
                && let Some(children) = self.children.get(&id)
            {
                stack.extend(children.iter().copied());
            }
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn build_marker(&self) -> usize {
        self.build_marker
    }
}

fn validate_entity_requirements(
    snapshot: &koharu_storage::Snapshot,
    entity: EntityId,
) -> Result<()> {
    let source_key = key::<SourceText>("default")?;
    let record = snapshot.record(entity.storage())?;
    if record
        .components()
        .any(|(key, _)| key.kind().as_str() == Translation::KIND)
        && record.component(&source_key).is_none()
    {
        Err(Error::invalid(format!(
            "entity {entity} has a translation but no source text"
        )))
    } else {
        Ok(())
    }
}

pub(crate) fn read<T: SceneComponent>(
    snapshot: &koharu_storage::Snapshot,
    record: koharu_storage::RecordId,
    slot: &str,
    context: &ValidationContext<'_>,
) -> Result<Option<T>> {
    let key = key::<T>(slot)?;
    snapshot
        .component(record, &key)?
        .map(|raw| decode::<T>(slot, raw, context))
        .transpose()
}

fn validate_known(
    _snapshot: &koharu_storage::Snapshot,
    _record: koharu_storage::RecordId,
    kind: &str,
    slot: &str,
    raw: &koharu_storage::ComponentRecord,
    context: &ValidationContext<'_>,
) -> Result<()> {
    if matches!(kind, Children::KIND | Page::KIND | Relation::KIND) && slot != "default" {
        return Err(Error::invalid(format!(
            "structural component {kind} must use the default slot"
        )));
    }
    macro_rules! validate {
        ($type:ty) => {
            if kind == <$type as SceneComponent>::KIND {
                decode::<$type>(slot, raw, context)?;
                return Ok(());
            }
        };
    }
    validate!(Children);
    validate!(ProjectSettings);
    validate!(Page);
    validate!(Geometry);
    validate!(Visibility);
    validate!(SourceText);
    validate!(OcrAnalysis);
    validate!(Translation);
    validate!(TextRole);
    validate!(ReadingOrder);
    validate!(Typography);
    validate!(Region);
    validate!(DetectionAnalysis);
    validate!(Asset);
    validate!(EntityOrigin);
    validate!(Relation);
    Ok(())
}
