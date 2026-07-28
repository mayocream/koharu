use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{
    BlobId, ComponentKey, ComponentRecord, Error, Patch, PatchId, RecordId, RecordRef, Result,
    blob::BlobAttachment,
    patch::{BaseRevision, Operation, PatchSegment},
    state::{State, StoredRecord},
};

pub struct Edit {
    base: Arc<State>,
    state: State,
    lineage: BTreeSet<PatchId>,
    attachments: BTreeMap<BlobId, Arc<[u8]>>,
    dirty_records: BTreeSet<RecordId>,
}

impl Edit {
    pub(crate) fn new(base: Arc<State>, lineage: BTreeSet<PatchId>) -> Self {
        Self {
            state: (*base).clone(),
            base,
            lineage,
            attachments: BTreeMap::new(),
            dirty_records: BTreeSet::new(),
        }
    }

    pub fn insert_record(&mut self) -> Result<RecordId> {
        let id = RecordId::new();
        self.insert_record_with_id(id)?;
        Ok(id)
    }

    pub fn insert_record_with_id(&mut self, id: RecordId) -> Result<()> {
        self.state.insert_record(id)?;
        self.dirty_records.insert(id);
        Ok(())
    }

    pub fn remove_record(&mut self, id: RecordId) -> Result<()> {
        self.state.remove_record(id)?;
        self.dirty_records.insert(id);
        Ok(())
    }

    pub fn set_component(
        &mut self,
        record: RecordId,
        key: ComponentKey,
        value: ComponentRecord,
    ) -> Result<()> {
        self.state.set_component(record, key, value)?;
        self.dirty_records.insert(record);
        Ok(())
    }

    pub fn remove_component(&mut self, record: RecordId, key: &ComponentKey) -> Result<()> {
        self.state.remove_component(record, key)?;
        self.dirty_records.insert(record);
        Ok(())
    }

    pub fn attach_blob(&mut self, bytes: impl Into<Arc<[u8]>>) -> BlobId {
        let attachment = BlobAttachment::new(bytes);
        let id = attachment.id();
        self.attachments
            .entry(id)
            .or_insert_with(|| attachment.bytes());
        id
    }

    #[must_use]
    pub fn view(&self) -> EditView<'_> {
        EditView { state: &self.state }
    }

    pub fn finish(mut self) -> Result<Patch> {
        #[cfg(debug_assertions)]
        self.state.validate()?;
        let operations = diff(&self.base, &self.state, &self.dirty_records)?;
        self.attachments
            .retain(|id, _| self.state.references_blob(*id));
        let base = BaseRevision {
            document: self.base.document,
            revision: self.base.revision,
        };
        let segment = PatchSegment::new(base, self.lineage, operations, self.attachments)?;
        Ok(Patch::from_segment(base, segment))
    }
}

#[derive(Copy, Clone)]
pub struct EditView<'edit> {
    state: &'edit State,
}

impl<'edit> EditView<'edit> {
    #[must_use]
    pub const fn root(self) -> RecordId {
        self.state.root
    }

    #[must_use]
    pub fn contains_record(self, id: RecordId) -> bool {
        self.state.records.contains_key(&id)
    }

    pub fn record(self, id: RecordId) -> Result<RecordRef<'edit>> {
        self.state.record(id)
    }

    pub fn records(self) -> impl Iterator<Item = RecordRef<'edit>> {
        self.state.records()
    }

    pub fn component(
        self,
        record: RecordId,
        key: &ComponentKey,
    ) -> Result<Option<&'edit ComponentRecord>> {
        self.state.component(record, key)
    }
}

fn diff(
    before: &State,
    after: &State,
    dirty_records: &BTreeSet<RecordId>,
) -> Result<Vec<Operation>> {
    if before.document != after.document || before.root != after.root {
        return Err(Error::invalid("editor changed immutable document identity"));
    }
    let before_ids = dirty_records
        .iter()
        .copied()
        .filter(|id| before.records.contains_key(id))
        .collect::<BTreeSet<_>>();
    let after_ids = dirty_records
        .iter()
        .copied()
        .filter(|id| after.records.contains_key(id))
        .collect::<BTreeSet<_>>();
    let inserted = after_ids
        .difference(&before_ids)
        .copied()
        .collect::<Vec<_>>();
    let removed = before_ids
        .difference(&after_ids)
        .copied()
        .collect::<Vec<_>>();
    let retained = before_ids
        .intersection(&after_ids)
        .copied()
        .collect::<Vec<_>>();
    let mut operations = Vec::new();

    // Create every identity before restoring components, so new records may
    // freely reference one another.
    for id in &inserted {
        operations.push(Operation::InsertRecord {
            record: StoredRecord {
                id: *id,
                components: Vec::new(),
            },
        });
    }

    for id in inserted.iter().chain(retained.iter()) {
        let before_record = before.records.get(id);
        let after_record = after.records.get(id).expect("record exists");
        let before_components = before_record
            .map(|record| {
                record
                    .components
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_stored()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let after_components = after_record
            .components
            .iter()
            .map(|(key, value)| (key.clone(), value.to_stored()))
            .collect::<BTreeMap<_, _>>();
        let keys = before_components
            .keys()
            .chain(after_components.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        for key in keys {
            let old = before_components.get(&key).cloned();
            let new = after_components.get(&key).cloned();
            if old != new {
                operations.push(Operation::ReplaceComponent {
                    record: *id,
                    key,
                    before: old,
                    after: new,
                });
            }
        }
    }

    // Clear components on every removed record first. This breaks reference
    // cycles and makes both forward replay and reverse replay dependency-safe.
    for id in &removed {
        let record = before.records.get(id).expect("record exists");
        for (key, value) in &record.components {
            operations.push(Operation::ReplaceComponent {
                record: *id,
                key: key.clone(),
                before: Some(value.to_stored()),
                after: None,
            });
        }
    }
    for id in &removed {
        operations.push(Operation::RemoveRecord {
            record: StoredRecord {
                id: *id,
                components: Vec::new(),
            },
        });
    }
    Ok(operations)
}
