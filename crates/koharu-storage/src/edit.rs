use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{
    BlobId, ComponentAddress, ComponentKey, ComponentRecord, Error, Patch, RecordId, RecordRef,
    Result,
    blob::BlobAttachment,
    patch::{Observation, Operation},
    state::{State, StoredRecord},
};

pub struct Edit {
    base: Arc<State>,
    state: State,
    attachments: BTreeMap<BlobId, Arc<[u8]>>,
    observations: BTreeSet<Observation>,
    dirty_lifecycles: BTreeSet<RecordId>,
    dirty_components: BTreeSet<ComponentAddress>,
}

impl Edit {
    pub(crate) fn new(base: Arc<State>) -> Self {
        Self {
            state: (*base).clone(),
            base,
            attachments: BTreeMap::new(),
            observations: BTreeSet::new(),
            dirty_lifecycles: BTreeSet::new(),
            dirty_components: BTreeSet::new(),
        }
    }

    pub fn insert_record(&mut self) -> Result<RecordId> {
        let id = RecordId::new();
        self.insert_record_with_id(id)?;
        Ok(id)
    }

    pub fn insert_record_with_id(&mut self, id: RecordId) -> Result<()> {
        self.state.insert_record(id)?;
        self.dirty_lifecycles.insert(id);
        Ok(())
    }

    pub fn remove_record(&mut self, id: RecordId) -> Result<()> {
        self.state.remove_record(id)?;
        self.dirty_lifecycles.insert(id);
        Ok(())
    }

    pub fn set_component(
        &mut self,
        record: RecordId,
        key: ComponentKey,
        value: ComponentRecord,
    ) -> Result<()> {
        self.state.set_component(record, key.clone(), value)?;
        self.dirty_components
            .insert(ComponentAddress { record, key });
        Ok(())
    }

    pub fn remove_component(&mut self, record: RecordId, key: &ComponentKey) -> Result<()> {
        self.state.remove_component(record, key)?;
        self.dirty_components.insert(ComponentAddress {
            record,
            key: key.clone(),
        });
        Ok(())
    }

    /// Records the existence and complete component state of `record` as an
    /// optimistic input to this edit. A later explicit rebase fails if any
    /// component on the record changed.
    pub fn observe_record(&mut self, record: RecordId) -> Result<()> {
        self.state.record(record)?;
        self.observations.insert(Observation::Record {
            record,
            fingerprint: self.base.record_fingerprint(record),
        });
        Ok(())
    }

    /// Records one component value (including absence) as an optimistic input
    /// to this edit. Ordinary writes already carry their own before-value;
    /// observations are for values that influenced a different write.
    pub fn observe_component(&mut self, record: RecordId, key: &ComponentKey) -> Result<()> {
        self.state.record(record)?;
        if !self.base.records.contains_key(&record) {
            return Ok(());
        }
        let fingerprint = self
            .base
            .component(record, key)?
            .map(|value| *value.fingerprint());
        self.observations.insert(Observation::Component {
            record,
            key: key.clone(),
            fingerprint,
        });
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
        let operations = diff(
            &self.base,
            &self.state,
            &self.dirty_lifecycles,
            &self.dirty_components,
        )?;
        self.attachments
            .retain(|id, _| self.state.references_blob(*id));
        Patch::new(
            self.base,
            self.observations.into_iter().collect(),
            operations,
            self.attachments,
            None,
        )
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
    dirty_lifecycles: &BTreeSet<RecordId>,
    dirty_components: &BTreeSet<ComponentAddress>,
) -> Result<Vec<Operation>> {
    if before.document != after.document || before.root != after.root {
        return Err(Error::invalid("editor changed immutable document identity"));
    }
    let before_ids = dirty_lifecycles
        .iter()
        .copied()
        .filter(|id| before.records.contains_key(id))
        .collect::<BTreeSet<_>>();
    let after_ids = dirty_lifecycles
        .iter()
        .copied()
        .filter(|id| after.records.contains_key(id))
        .collect::<BTreeSet<_>>();
    let inserted = after_ids
        .difference(&before_ids)
        .copied()
        .collect::<BTreeSet<_>>();
    let removed = before_ids
        .difference(&after_ids)
        .copied()
        .collect::<BTreeSet<_>>();
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

    for id in &inserted {
        let record = after.records.get(id).expect("inserted record exists");
        for (key, value) in &record.components {
            operations.push(Operation::ReplaceComponent {
                record: *id,
                key: key.clone(),
                before: None,
                after: Some(value.to_stored()),
            });
        }
    }

    for address in dirty_components {
        if inserted.contains(&address.record) || removed.contains(&address.record) {
            continue;
        }
        let old = before
            .records
            .get(&address.record)
            .and_then(|record| record.components.get(&address.key));
        let new = after
            .records
            .get(&address.record)
            .and_then(|record| record.components.get(&address.key));
        if old != new {
            operations.push(Operation::ReplaceComponent {
                record: address.record,
                key: address.key.clone(),
                before: old.map(ComponentRecord::to_stored),
                after: new.map(ComponentRecord::to_stored),
            });
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
