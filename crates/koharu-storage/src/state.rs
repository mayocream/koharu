use std::collections::{BTreeMap, BTreeSet};

use imbl::{HashMap, OrdMap, OrdSet};
use revision::revisioned;

use crate::{
    BlobId, ComponentAddress, ComponentKey, ComponentRecord, DocumentId, Error, RecordId, Result,
    Revision,
    component::{StoredComponent, StoredComponentEntry},
};

type RebuiltIndexes = (
    HashMap<RecordId, OrdSet<ComponentAddress>>,
    HashMap<BlobId, u64>,
);

const MAX_RECORDS: usize = 10_000_000;
const MAX_COMPONENTS_PER_RECORD: usize = 1_000_000;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Record {
    pub(crate) components: OrdMap<ComponentKey, ComponentRecord>,
}

#[derive(Clone, Debug)]
pub(crate) struct State {
    pub(crate) document: DocumentId,
    pub(crate) revision: Revision,
    pub(crate) root: RecordId,
    pub(crate) records: HashMap<RecordId, Record>,
    pub(crate) incoming_refs: HashMap<RecordId, OrdSet<ComponentAddress>>,
    pub(crate) blob_counts: HashMap<BlobId, u64>,
}

#[derive(Copy, Clone)]
pub struct RecordRef<'snapshot> {
    id: RecordId,
    record: &'snapshot Record,
}

impl<'snapshot> RecordRef<'snapshot> {
    #[must_use]
    pub const fn id(self) -> RecordId {
        self.id
    }

    #[must_use]
    pub fn len(self) -> usize {
        self.record.components.len()
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.record.components.is_empty()
    }

    pub fn components(
        self,
    ) -> impl ExactSizeIterator<Item = (&'snapshot ComponentKey, &'snapshot ComponentRecord)> {
        self.record.components.iter()
    }

    #[must_use]
    pub fn component(self, key: &ComponentKey) -> Option<&'snapshot ComponentRecord> {
        self.record.components.get(key)
    }
}

impl std::fmt::Debug for RecordRef<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecordRef")
            .field("id", &self.id)
            .field("components", &self.record.components.len())
            .finish()
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredRecord {
    pub(crate) id: RecordId,
    pub(crate) components: Vec<StoredComponentEntry>,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CheckpointRecord {
    pub(crate) root: RecordId,
    pub(crate) records: Vec<StoredRecord>,
}

impl State {
    pub(crate) fn empty(document: DocumentId) -> Self {
        let root = RecordId::new();
        let mut records = HashMap::new();
        records.insert(root, Record::default());
        Self {
            document,
            revision: Revision::ZERO,
            root,
            records,
            incoming_refs: HashMap::new(),
            blob_counts: HashMap::new(),
        }
    }

    pub(crate) fn from_checkpoint(
        document: DocumentId,
        revision: Revision,
        checkpoint: CheckpointRecord,
    ) -> Result<Self> {
        if checkpoint.records.is_empty() {
            return Err(Error::invalid("checkpoint contains no records"));
        }
        if checkpoint.records.len() > MAX_RECORDS {
            return Err(Error::invalid("checkpoint contains too many records"));
        }
        let mut state = Self {
            document,
            revision,
            root: checkpoint.root,
            records: HashMap::new(),
            incoming_refs: HashMap::new(),
            blob_counts: HashMap::new(),
        };
        for record in &checkpoint.records {
            if state.records.insert(record.id, Record::default()).is_some() {
                return Err(Error::invalid("checkpoint contains duplicate record IDs"));
            }
        }
        if !state.records.contains_key(&state.root) {
            return Err(Error::invalid("checkpoint root record is missing"));
        }
        for record in checkpoint.records {
            let mut previous = None;
            for component in record.components {
                if previous.as_ref().is_some_and(|key| key >= &component.key) {
                    return Err(Error::invalid(
                        "checkpoint components are not uniquely sorted",
                    ));
                }
                previous = Some(component.key.clone());
                state.set_component(
                    record.id,
                    component.key,
                    ComponentRecord::from_stored(component.value)?,
                )?;
            }
        }
        state.validate()?;
        Ok(state)
    }

    pub(crate) fn to_checkpoint(&self) -> CheckpointRecord {
        let mut ids = self.records.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable();
        let records = ids
            .into_iter()
            .map(|id| self.stored_record(id).expect("known record"))
            .collect();
        CheckpointRecord {
            root: self.root,
            records,
        }
    }

    pub(crate) fn record(&self, id: RecordId) -> Result<RecordRef<'_>> {
        self.records
            .get(&id)
            .map(|record| RecordRef { id, record })
            .ok_or(Error::RecordNotFound(id))
    }

    pub(crate) fn records(&self) -> impl Iterator<Item = RecordRef<'_>> {
        self.records
            .iter()
            .map(|(id, record)| RecordRef { id: *id, record })
    }

    pub(crate) fn component(
        &self,
        record: RecordId,
        key: &ComponentKey,
    ) -> Result<Option<&ComponentRecord>> {
        Ok(self.record(record)?.record.components.get(key))
    }

    pub(crate) fn incoming(
        &self,
        record: RecordId,
    ) -> Result<impl Iterator<Item = &ComponentAddress>> {
        if !self.records.contains_key(&record) {
            return Err(Error::RecordNotFound(record));
        }
        Ok(self
            .incoming_refs
            .get(&record)
            .into_iter()
            .flat_map(OrdSet::iter))
    }

    pub(crate) fn insert_record(&mut self, id: RecordId) -> Result<()> {
        if self.records.len() >= MAX_RECORDS {
            return Err(Error::invalid("document record limit was reached"));
        }
        if self.records.contains_key(&id) {
            return Err(Error::RecordAlreadyExists(id));
        }
        self.records.insert(id, Record::default());
        Ok(())
    }

    pub(crate) fn insert_stored_record(&mut self, record: StoredRecord) -> Result<()> {
        let id = record.id;
        if !record
            .components
            .windows(2)
            .all(|pair| pair[0].key < pair[1].key)
        {
            return Err(Error::invalid(
                "stored record components are not canonically sorted",
            ));
        }
        self.insert_record(id)?;
        for component in record.components {
            self.set_component(
                id,
                component.key,
                ComponentRecord::from_stored(component.value)?,
            )?;
        }
        Ok(())
    }

    pub(crate) fn remove_record(&mut self, id: RecordId) -> Result<StoredRecord> {
        if id == self.root {
            return Err(Error::RootRemoval);
        }
        let record = self
            .records
            .get(&id)
            .cloned()
            .ok_or(Error::RecordNotFound(id))?;
        let external_references = self
            .incoming_refs
            .get(&id)
            .map(|addresses| {
                addresses
                    .iter()
                    .filter(|address| address.record != id)
                    .count()
            })
            .unwrap_or(0);
        if external_references != 0 {
            return Err(Error::RecordReferenced {
                record: id,
                count: external_references,
            });
        }
        let stored = stored_record(id, &record);
        for (key, value) in record.components.iter() {
            self.remove_indexes(
                &ComponentAddress {
                    record: id,
                    key: key.clone(),
                },
                value,
            )?;
        }
        self.records.remove(&id);
        self.incoming_refs.remove(&id);
        Ok(stored)
    }

    pub(crate) fn set_component(
        &mut self,
        record: RecordId,
        key: ComponentKey,
        value: ComponentRecord,
    ) -> Result<Option<ComponentRecord>> {
        if !self.records.contains_key(&record) {
            return Err(Error::RecordNotFound(record));
        }
        for referenced in value.record_refs() {
            if !self.records.contains_key(referenced) {
                return Err(Error::RecordNotFound(*referenced));
            }
        }
        let address = ComponentAddress {
            record,
            key: key.clone(),
        };
        let previous = self
            .records
            .get(&record)
            .and_then(|record| record.components.get(&key))
            .cloned();
        if previous.is_none()
            && self
                .records
                .get(&record)
                .is_some_and(|record| record.components.len() >= MAX_COMPONENTS_PER_RECORD)
        {
            return Err(Error::invalid("record component limit was reached"));
        }
        if let Some(previous) = &previous {
            self.remove_indexes(&address, previous)?;
        }
        self.records
            .get_mut(&record)
            .expect("record checked above")
            .components
            .insert(key, value.clone());
        self.add_indexes(address, &value)?;
        Ok(previous)
    }

    pub(crate) fn remove_component(
        &mut self,
        record: RecordId,
        key: &ComponentKey,
    ) -> Result<Option<ComponentRecord>> {
        let Some(value) = self
            .records
            .get(&record)
            .ok_or(Error::RecordNotFound(record))?
            .components
            .get(key)
            .cloned()
        else {
            return Ok(None);
        };
        self.remove_indexes(
            &ComponentAddress {
                record,
                key: key.clone(),
            },
            &value,
        )?;
        self.records
            .get_mut(&record)
            .expect("record checked above")
            .components
            .remove(key);
        Ok(Some(value))
    }

    pub(crate) fn stored_record(&self, id: RecordId) -> Result<StoredRecord> {
        let record = self.records.get(&id).ok_or(Error::RecordNotFound(id))?;
        Ok(stored_record(id, record))
    }

    pub(crate) fn record_fingerprint(&self, id: RecordId) -> Option<[u8; 32]> {
        let record = self.records.get(&id)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"koharu-storage-record-v1");
        hasher.update(id.as_uuid().as_bytes());
        hasher.update(&(record.components.len() as u64).to_le_bytes());
        for (key, value) in &record.components {
            hasher.update(&(key.kind().as_str().len() as u64).to_le_bytes());
            hasher.update(key.kind().as_str().as_bytes());
            hasher.update(&(key.slot().as_str().len() as u64).to_le_bytes());
            hasher.update(key.slot().as_str().as_bytes());
            hasher.update(value.fingerprint());
        }
        Some(*hasher.finalize().as_bytes())
    }

    pub(crate) fn referenced_blobs(&self) -> BTreeSet<BlobId> {
        self.blob_counts.keys().copied().collect()
    }

    pub(crate) fn references_blob(&self, id: BlobId) -> bool {
        self.blob_counts.contains_key(&id)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !self.records.contains_key(&self.root) {
            return Err(Error::invalid("root record is missing"));
        }
        let (incoming_refs, blob_counts) = self.rebuild_indexes()?;
        if incoming_refs != self.incoming_refs {
            return Err(Error::invalid("reverse-reference index is inconsistent"));
        }
        if blob_counts != self.blob_counts {
            return Err(Error::invalid("blob-reference index is inconsistent"));
        }
        Ok(())
    }

    fn rebuild_indexes(&self) -> Result<RebuiltIndexes> {
        let mut incoming_refs = HashMap::<RecordId, OrdSet<ComponentAddress>>::new();
        let mut blob_counts = HashMap::<BlobId, u64>::new();
        for (record_id, record) in &self.records {
            for (key, value) in &record.components {
                let address = ComponentAddress {
                    record: *record_id,
                    key: key.clone(),
                };
                for referenced in value.record_refs() {
                    if !self.records.contains_key(referenced) {
                        return Err(Error::RecordNotFound(*referenced));
                    }
                    incoming_refs
                        .entry(*referenced)
                        .or_default()
                        .insert(address.clone());
                }
                for blob in value.blob_refs() {
                    let count = blob_counts.get(blob).copied().unwrap_or(0);
                    blob_counts.insert(
                        *blob,
                        count
                            .checked_add(1)
                            .ok_or_else(|| Error::invalid("blob reference count overflow"))?,
                    );
                }
            }
        }
        Ok((incoming_refs, blob_counts))
    }

    fn add_indexes(&mut self, address: ComponentAddress, value: &ComponentRecord) -> Result<()> {
        for referenced in value.record_refs() {
            self.incoming_refs
                .entry(*referenced)
                .or_default()
                .insert(address.clone());
        }
        for blob in value.blob_refs() {
            let count = self.blob_counts.get(blob).copied().unwrap_or(0);
            self.blob_counts.insert(
                *blob,
                count
                    .checked_add(1)
                    .ok_or_else(|| Error::invalid("blob reference count overflow"))?,
            );
        }
        Ok(())
    }

    fn remove_indexes(
        &mut self,
        address: &ComponentAddress,
        value: &ComponentRecord,
    ) -> Result<()> {
        for referenced in value.record_refs() {
            if let Some(addresses) = self.incoming_refs.get_mut(referenced) {
                addresses.remove(address);
                if addresses.is_empty() {
                    self.incoming_refs.remove(referenced);
                }
            }
        }
        for blob in value.blob_refs() {
            let count = self
                .blob_counts
                .get(blob)
                .copied()
                .ok_or_else(|| Error::invalid("blob reference count underflow"))?;
            if count == 1 {
                self.blob_counts.remove(blob);
            } else {
                self.blob_counts.insert(*blob, count - 1);
            }
        }
        Ok(())
    }

    pub(crate) fn semantic_map(&self) -> BTreeMap<RecordId, StoredRecord> {
        self.records
            .keys()
            .copied()
            .map(|id| (id, self.stored_record(id).expect("known record")))
            .collect()
    }
}

fn stored_record(id: RecordId, record: &Record) -> StoredRecord {
    StoredRecord {
        id,
        components: record
            .components
            .iter()
            .map(|(key, value)| StoredComponentEntry {
                key: key.clone(),
                value: value.to_stored(),
            })
            .collect(),
    }
}

impl From<ComponentRecord> for StoredComponent {
    fn from(value: ComponentRecord) -> Self {
        value.to_stored()
    }
}
