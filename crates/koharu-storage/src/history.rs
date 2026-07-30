use std::collections::{BTreeMap, BTreeSet};

use revision::revisioned;

use crate::{
    BlobId, ComponentAddress, ComponentKey, RecordId, Revision,
    patch::Operation,
    state::{CheckpointRecord, State},
};

#[revisioned(revision = 1)]
#[derive(Clone, Debug)]
pub(crate) struct Checkpoint {
    pub(crate) document: CheckpointRecord,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug)]
pub(crate) struct StoredCommit {
    pub(crate) label: Option<String>,
    pub(crate) operations: Vec<Operation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeSet {
    pub from: Revision,
    pub to: Revision,
    pub records: Vec<RecordChange>,
    pub components: Vec<ComponentChange>,
    pub blobs_added: Vec<BlobId>,
}

impl ChangeSet {
    pub(crate) fn from_operations<'a>(
        from: Revision,
        to: Revision,
        operations: impl IntoIterator<Item = &'a Operation>,
        blobs_added: impl IntoIterator<Item = BlobId>,
    ) -> Self {
        let mut record_states = BTreeMap::<RecordId, (bool, bool)>::new();
        let mut component_states = BTreeMap::<
            ComponentAddress,
            (
                Option<crate::component::StoredComponent>,
                Option<crate::component::StoredComponent>,
            ),
        >::new();

        for operation in operations {
            match operation {
                Operation::InsertRecord { record } => {
                    record_states
                        .entry(record.id)
                        .and_modify(|state| state.1 = true)
                        .or_insert((false, true));
                }
                Operation::RemoveRecord { record } => {
                    record_states
                        .entry(record.id)
                        .and_modify(|state| state.1 = false)
                        .or_insert((true, false));
                }
                Operation::ReplaceComponent {
                    record,
                    key,
                    before,
                    after,
                } => {
                    component_states
                        .entry(ComponentAddress {
                            record: *record,
                            key: key.clone(),
                        })
                        .and_modify(|state| state.1 = after.clone())
                        .or_insert_with(|| (before.clone(), after.clone()));
                }
            }
        }

        let records = record_states
            .into_iter()
            .filter_map(|(id, (before, after))| match (before, after) {
                (false, true) => Some(RecordChange::Inserted(id)),
                (true, false) => Some(RecordChange::Removed(id)),
                _ => None,
            })
            .collect();
        let components = component_states
            .into_iter()
            .filter_map(|(address, (before, after))| {
                let kind = match (&before, &after) {
                    (None, Some(_)) => Some(ValueChangeKind::Inserted),
                    (Some(_), None) => Some(ValueChangeKind::Removed),
                    (Some(left), Some(right)) if left != right => Some(ValueChangeKind::Replaced),
                    _ => None,
                }?;
                Some(ComponentChange { address, kind })
            })
            .collect();
        let mut blobs_added = blobs_added.into_iter().collect::<Vec<_>>();
        blobs_added.sort_unstable();
        blobs_added.dedup();
        Self {
            from,
            to,
            records,
            components,
            blobs_added,
        }
    }

    pub(crate) fn between(
        before: &State,
        after: &State,
        blobs_added: impl IntoIterator<Item = BlobId>,
    ) -> Self {
        let old = before.semantic_map();
        let new = after.semantic_map();
        let old_ids = old.keys().copied().collect::<BTreeSet<_>>();
        let new_ids = new.keys().copied().collect::<BTreeSet<_>>();
        let mut records = old_ids
            .difference(&new_ids)
            .copied()
            .map(RecordChange::Removed)
            .chain(
                new_ids
                    .difference(&old_ids)
                    .copied()
                    .map(RecordChange::Inserted),
            )
            .collect::<Vec<_>>();
        records.sort_by_key(|change| change.id());

        let mut components = Vec::new();
        for id in old_ids.union(&new_ids).copied() {
            let old_components = old.get(&id).map(component_map).unwrap_or_default();
            let new_components = new.get(&id).map(component_map).unwrap_or_default();
            let keys = old_components
                .keys()
                .chain(new_components.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for key in keys {
                let old_value = old_components.get(&key);
                let new_value = new_components.get(&key);
                let kind = match (old_value, new_value) {
                    (None, Some(_)) => Some(ValueChangeKind::Inserted),
                    (Some(_), None) => Some(ValueChangeKind::Removed),
                    (Some(old), Some(new)) if old != new => Some(ValueChangeKind::Replaced),
                    _ => None,
                };
                if let Some(kind) = kind {
                    components.push(ComponentChange {
                        address: ComponentAddress { record: id, key },
                        kind,
                    });
                }
            }
        }
        components.sort_by(|left, right| left.address.cmp(&right.address));
        let mut blobs_added = blobs_added.into_iter().collect::<Vec<_>>();
        blobs_added.sort_unstable();
        blobs_added.dedup();
        Self {
            from: before.revision,
            to: after.revision,
            records,
            components,
            blobs_added,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RecordChange {
    Inserted(RecordId),
    Removed(RecordId),
}

impl RecordChange {
    #[must_use]
    pub const fn id(self) -> RecordId {
        match self {
            Self::Inserted(id) | Self::Removed(id) => id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentChange {
    pub address: ComponentAddress,
    pub kind: ValueChangeKind,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ValueChangeKind {
    Inserted,
    Removed,
    Replaced,
}

fn component_map(
    record: &crate::state::StoredRecord,
) -> BTreeMap<ComponentKey, crate::component::StoredComponent> {
    record
        .components
        .iter()
        .map(|entry| (entry.key.clone(), entry.value.clone()))
        .collect()
}
