use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use revision::revisioned;

use crate::{
    BlobId, ComponentKey, ComponentRecord, DocumentId, Error, PatchId, RecordId, Result, Revision,
    component::StoredComponent,
    state::{State, StoredRecord},
};

type AppliedPatch = (State, BTreeMap<BlobId, Arc<[u8]>>);

#[revisioned(revision = 1)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BaseRevision {
    pub document: DocumentId,
    pub revision: Revision,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Operation {
    InsertRecord {
        record: StoredRecord,
    },
    RemoveRecord {
        record: StoredRecord,
    },
    ReplaceComponent {
        record: RecordId,
        key: ComponentKey,
        before: Option<StoredComponent>,
        after: Option<StoredComponent>,
    },
}

impl Operation {
    pub(crate) fn apply(&self, state: &mut State) -> Result<()> {
        match self {
            Self::InsertRecord { record } => state.insert_stored_record(record.clone()),
            Self::RemoveRecord { record } => {
                let current = state.stored_record(record.id)?;
                if current != *record {
                    return Err(Error::patch_conflict(format!(
                        "record {} changed before removal",
                        record.id
                    )));
                }
                state.remove_record(record.id).map(|_| ())
            }
            Self::ReplaceComponent {
                record,
                key,
                before,
                after,
            } => {
                let current = state
                    .component(*record, key)?
                    .map(ComponentRecord::to_stored);
                if current != *before {
                    return Err(Error::patch_conflict(format!(
                        "component {record}/{key} changed"
                    )));
                }
                match after {
                    Some(value) => state
                        .set_component(
                            *record,
                            key.clone(),
                            ComponentRecord::from_stored(value.clone())?,
                        )
                        .map(|_| ()),
                    None => state.remove_component(*record, key).map(|_| ()),
                }
            }
        }
    }

    pub(crate) fn reversed(&self) -> Self {
        match self {
            Self::InsertRecord { record } => Self::RemoveRecord {
                record: record.clone(),
            },
            Self::RemoveRecord { record } => Self::InsertRecord {
                record: record.clone(),
            },
            Self::ReplaceComponent {
                record,
                key,
                before,
                after,
            } => Self::ReplaceComponent {
                record: *record,
                key: key.clone(),
                before: after.clone(),
                after: before.clone(),
            },
        }
    }

    pub(crate) fn blob_refs(&self, blobs: &mut BTreeSet<BlobId>) {
        match self {
            Self::InsertRecord { record } | Self::RemoveRecord { record } => {
                for component in &record.components {
                    blobs.extend(component.value.blob_refs.iter().copied());
                }
            }
            Self::ReplaceComponent { before, after, .. } => {
                for value in [before, after].into_iter().flatten() {
                    blobs.extend(value.blob_refs.iter().copied());
                }
            }
        }
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Observation {
    Record {
        record: RecordId,
        fingerprint: Option<[u8; 32]>,
    },
    Component {
        record: RecordId,
        key: ComponentKey,
        fingerprint: Option<[u8; 32]>,
    },
}

impl Observation {
    fn validate(&self, state: &State) -> Result<()> {
        match self {
            Self::Record {
                record,
                fingerprint,
            } => {
                if state.record_fingerprint(*record) != *fingerprint {
                    return Err(Error::patch_conflict(format!(
                        "observed record {record} changed"
                    )));
                }
            }
            Self::Component {
                record,
                key,
                fingerprint,
            } => {
                let current = state
                    .component(*record, key)
                    .map_err(|_| {
                        Error::patch_conflict(format!(
                            "record {record} containing observed component {key} changed"
                        ))
                    })?
                    .map(|value| *value.fingerprint());
                if current != *fingerprint {
                    return Err(Error::patch_conflict(format!(
                        "observed component {record}/{key} changed"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PatchEffects {
    records: BTreeSet<RecordId>,
    components: BTreeSet<crate::ComponentAddress>,
}

impl PatchEffects {
    #[doc(hidden)]
    #[must_use]
    pub fn from_changes(changes: &crate::ChangeSet) -> Self {
        Self {
            records: changes.records.iter().map(|change| change.id()).collect(),
            components: changes
                .components
                .iter()
                .map(|change| change.address.clone())
                .collect(),
        }
    }

    #[must_use]
    pub fn records(&self) -> impl ExactSizeIterator<Item = RecordId> + '_ {
        self.records.iter().copied()
    }

    #[must_use]
    pub fn components(&self) -> impl ExactSizeIterator<Item = &crate::ComponentAddress> {
        self.components.iter()
    }

    #[must_use]
    pub fn changes_record_lifecycle(&self) -> bool {
        !self.records.is_empty()
    }
}

#[revisioned(revision = 1)]
#[derive(Clone)]
struct PatchIdentity {
    base: BaseRevision,
    observations: Vec<Observation>,
    operations: Vec<Operation>,
    attachments: Vec<BlobId>,
}

#[revisioned(revision = 1)]
#[derive(Clone)]
struct PatchFingerprintIdentity {
    body_id: PatchId,
    label: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Patch {
    base: BaseRevision,
    base_state: Arc<State>,
    observations: Arc<[Observation]>,
    operations: Arc<[Operation]>,
    attachments: Arc<BTreeMap<BlobId, Arc<[u8]>>>,
    body_id: PatchId,
    label: Option<Arc<str>>,
}

impl Patch {
    pub(crate) fn new(
        base_state: Arc<State>,
        observations: Vec<Observation>,
        operations: Vec<Operation>,
        attachments: BTreeMap<BlobId, Arc<[u8]>>,
        label: Option<Arc<str>>,
    ) -> Result<Self> {
        let base = BaseRevision {
            document: base_state.document,
            revision: base_state.revision,
        };
        let identity = PatchIdentity {
            base,
            observations: observations.clone(),
            operations: operations.clone(),
            attachments: attachments.keys().copied().collect(),
        };
        let body_id = PatchId::for_bytes(&revision::to_vec(&identity)?);
        Ok(Self {
            base,
            base_state,
            observations: observations.into(),
            operations: operations.into(),
            attachments: Arc::new(attachments),
            body_id,
            label,
        })
    }

    pub(crate) fn from_operations(
        base_state: Arc<State>,
        operations: Vec<Operation>,
        label: Option<Arc<str>>,
    ) -> Result<Self> {
        Self::new(base_state, Vec::new(), operations, BTreeMap::new(), label)
    }

    #[must_use]
    pub const fn base(&self) -> BaseRevision {
        self.base
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    #[must_use]
    pub fn with_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    #[must_use]
    pub fn fingerprint(&self) -> PatchId {
        let identity = PatchFingerprintIdentity {
            body_id: self.body_id,
            label: self.label.as_deref().map(str::to_owned),
        };
        let bytes = revision::to_vec(&identity)
            .expect("serializing a patch fingerprint into a byte vector cannot fail");
        PatchId::for_bytes(&bytes)
    }

    #[must_use]
    pub fn effects(&self) -> PatchEffects {
        let mut effects = PatchEffects::default();
        for operation in self.operations.iter() {
            match operation {
                Operation::InsertRecord { record } | Operation::RemoveRecord { record } => {
                    effects.records.insert(record.id);
                }
                Operation::ReplaceComponent { record, key, .. } => {
                    effects.components.insert(crate::ComponentAddress {
                        record: *record,
                        key: key.clone(),
                    });
                }
            }
        }
        effects
    }

    #[doc(hidden)]
    #[must_use]
    pub fn has_exact_input(&self, snapshot: &crate::Snapshot) -> bool {
        self.base.document == snapshot.document_id()
            && self.base.revision == snapshot.revision()
            && Arc::ptr_eq(&self.base_state, &snapshot.state)
    }

    /// Rebinds a patch to `snapshot` after verifying every observed input and
    /// write precondition. This is an explicit optimistic rebase; ordinary
    /// commits continue to reject stale revisions.
    pub fn rebase_on(&self, snapshot: &crate::Snapshot) -> Result<Self> {
        if self.base.document != snapshot.document_id() {
            return Err(Error::DocumentMismatch {
                patch: self.base.document,
                session: snapshot.document_id(),
            });
        }
        if self.has_exact_input(snapshot) {
            return Ok(self.clone());
        }

        let rebased = Self::new(
            snapshot.state.clone(),
            self.observations.to_vec(),
            self.operations.to_vec(),
            (*self.attachments).clone(),
            self.label.clone(),
        )?;
        snapshot.preview([&rebased])?;
        Ok(rebased)
    }

    pub(crate) fn apply(&self, state: &State) -> Result<AppliedPatch> {
        if state.document != self.base.document {
            return Err(Error::DocumentMismatch {
                patch: self.base.document,
                session: state.document,
            });
        }
        if state.revision != self.base.revision {
            return Err(Error::RevisionConflict {
                expected: self.base.revision,
                actual: state.revision,
            });
        }
        for observation in self.observations.iter() {
            observation.validate(state)?;
        }

        let mut next = state.clone();
        for operation in self.operations.iter() {
            operation.apply(&mut next)?;
        }
        for (id, bytes) in self.attachments.iter() {
            if BlobId::for_bytes(bytes) != *id {
                return Err(Error::invalid("patch attachment hash mismatch"));
            }
        }
        #[cfg(debug_assertions)]
        next.validate()?;
        Ok((next, (*self.attachments).clone()))
    }

    pub(crate) fn operations(&self) -> impl Iterator<Item = &Operation> {
        self.operations.iter()
    }
}
