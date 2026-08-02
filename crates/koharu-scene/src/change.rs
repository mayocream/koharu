use std::collections::{BTreeMap, BTreeSet};

use revision::revisioned;

use crate::{EntityId, RelationId, Revision, patch::Operation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    pub from: Revision,
    pub to: Revision,
    pub entities: Vec<EntityChange>,
    /// Entities and page roots whose hierarchy changed.
    pub hierarchy: Vec<EntityId>,
    pub components: Vec<ComponentChange>,
    pub relations: Vec<RelationChange>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EntityChange {
    Inserted(EntityId),
    Removed(EntityId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentChange {
    pub owner: ComponentOwner,
    pub kind: String,
    pub change: ValueChangeKind,
}

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ComponentOwner {
    Project,
    Entity(EntityId),
    Relation(RelationId),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ValueChangeKind {
    Inserted,
    Removed,
    Replaced,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RelationChange {
    Inserted(RelationId),
    Removed(RelationId),
    Changed(RelationId),
}

impl Change {
    pub(crate) fn empty(revision: Revision) -> Self {
        Self {
            from: revision,
            to: revision,
            entities: Vec::new(),
            hierarchy: Vec::new(),
            components: Vec::new(),
            relations: Vec::new(),
        }
    }

    pub(crate) fn from_operations(from: Revision, to: Revision, operations: &[Operation]) -> Self {
        let mut entities = BTreeMap::<EntityId, (bool, bool)>::new();
        let mut hierarchy = BTreeSet::new();
        let mut components = BTreeMap::new();
        let mut relations = BTreeMap::<RelationId, RelationChange>::new();

        for operation in operations {
            match operation {
                Operation::InsertPage { id, .. } => {
                    entities
                        .entry(*id)
                        .and_modify(|state| state.1 = true)
                        .or_insert((false, true));
                    hierarchy.insert(*id);
                }
                Operation::InsertEntity { page, id, .. } => {
                    entities
                        .entry(*id)
                        .and_modify(|state| state.1 = true)
                        .or_insert((false, true));
                    hierarchy.extend([*page, *id]);
                }
                Operation::RemovePage { id, .. } => {
                    entities
                        .entry(*id)
                        .and_modify(|state| state.1 = false)
                        .or_insert((true, false));
                    hierarchy.insert(*id);
                }
                Operation::RemoveEntity { page, id, .. } => {
                    entities
                        .entry(*id)
                        .and_modify(|state| state.1 = false)
                        .or_insert((true, false));
                    hierarchy.extend([*page, *id]);
                }
                Operation::MovePage { id, .. } => {
                    hierarchy.insert(*id);
                }
                Operation::MoveEntity {
                    id,
                    before_page,
                    before_parent,
                    after_page,
                    after_parent,
                    ..
                } => {
                    hierarchy.extend([
                        *id,
                        *before_page,
                        *before_parent,
                        *after_page,
                        *after_parent,
                    ]);
                }
                Operation::ReplaceComponent {
                    owner,
                    key,
                    before,
                    after,
                } => {
                    let change = match (before, after) {
                        (None, Some(_)) => ValueChangeKind::Inserted,
                        (Some(_), None) => ValueChangeKind::Removed,
                        _ => ValueChangeKind::Replaced,
                    };
                    components.insert(
                        (*owner, key.kind.clone()),
                        ComponentChange {
                            owner: *owner,
                            kind: key.kind.clone(),
                            change,
                        },
                    );
                }
                Operation::InsertRelation { id, .. } => {
                    relations.insert(*id, RelationChange::Inserted(*id));
                }
                Operation::RemoveRelation { id, .. } => {
                    relations.insert(*id, RelationChange::Removed(*id));
                }
                Operation::ReplaceRelation { id, .. } => {
                    relations.entry(*id).or_insert(RelationChange::Changed(*id));
                }
            }
        }

        let entities = entities
            .into_iter()
            .filter_map(|(id, state)| match state {
                (false, true) => Some(EntityChange::Inserted(id)),
                (true, false) => Some(EntityChange::Removed(id)),
                _ => None,
            })
            .collect();
        Self {
            from,
            to,
            entities,
            hierarchy: hierarchy.into_iter().collect(),
            components: components.into_values().collect(),
            relations: relations.into_values().collect(),
        }
    }
}
