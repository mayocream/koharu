use crate::{EntityId, RelationId, Revision};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneChangeSet {
    pub from: Revision,
    pub to: Revision,
    pub entities: Vec<EntityChange>,
    pub components: Vec<ComponentChange>,
    pub relations: Vec<RelationChange>,
    pub pages_changed: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EntityChange {
    Inserted(EntityId),
    Removed(EntityId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentChange {
    pub entity: EntityId,
    pub kind: String,
    pub slot: String,
    pub change: koharu_storage::ValueChangeKind,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RelationChange {
    Inserted(RelationId),
    Removed(RelationId),
    Changed(RelationId),
}

impl SceneChangeSet {
    pub(crate) fn from_storage(
        changes: &koharu_storage::ChangeSet,
        before: &crate::SceneSnapshot,
        after: &crate::SceneSnapshot,
    ) -> Self {
        let mut entities = Vec::new();
        let mut relations = Vec::new();
        for change in &changes.records {
            match *change {
                koharu_storage::RecordChange::Inserted(id) => {
                    let relation = RelationId::from_storage(id);
                    if after.index.relations.contains_key(&relation) {
                        relations.push(RelationChange::Inserted(relation));
                    } else {
                        entities.push(EntityChange::Inserted(EntityId::from_storage(id)));
                    }
                }
                koharu_storage::RecordChange::Removed(id) => {
                    let relation = RelationId::from_storage(id);
                    if before.index.relations.contains_key(&relation) {
                        relations.push(RelationChange::Removed(relation));
                    } else {
                        entities.push(EntityChange::Removed(EntityId::from_storage(id)));
                    }
                }
            }
        }
        let relation_kind = <crate::Relation as crate::SceneComponent>::KIND;
        let children_kind = <crate::Children as crate::SceneComponent>::KIND;
        let mut pages_changed = false;
        let mut components = Vec::new();
        for change in &changes.components {
            let address = &change.address;
            let kind = address.key.kind().as_str();
            if address.record == before.storage.root() && kind == children_kind {
                pages_changed = true;
            }
            let relation = RelationId::from_storage(address.record);
            if kind == relation_kind
                && (before.index.relations.contains_key(&relation)
                    || after.index.relations.contains_key(&relation))
            {
                if !relations.iter().any(|item| match item {
                    RelationChange::Inserted(id)
                    | RelationChange::Removed(id)
                    | RelationChange::Changed(id) => *id == relation,
                }) {
                    relations.push(RelationChange::Changed(relation));
                }
            } else if address.record != before.storage.root() {
                components.push(ComponentChange {
                    entity: EntityId::from_storage(address.record),
                    kind: kind.to_owned(),
                    slot: address.key.slot().as_str().to_owned(),
                    change: change.kind,
                });
            }
        }
        entities.sort_by_key(|change| match change {
            EntityChange::Inserted(id) | EntityChange::Removed(id) => *id,
        });
        relations.sort_by_key(|change| match change {
            RelationChange::Inserted(id)
            | RelationChange::Removed(id)
            | RelationChange::Changed(id) => *id,
        });
        components.sort_by(|left, right| {
            (&left.entity, &left.kind, &left.slot).cmp(&(&right.entity, &right.kind, &right.slot))
        });
        Self {
            from: changes.from,
            to: changes.to,
            entities,
            components,
            relations,
            pages_changed,
        }
    }
}
