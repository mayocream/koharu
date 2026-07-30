use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use revision::revisioned;

use crate::{
    BlobId, ComponentKey, ComponentRecord, DocumentId, Error, PatchId, RecordId, Result, Revision,
    blob::BlobAttachment,
    component::StoredComponent,
    state::{State, StoredRecord},
};

type AppliedPatch = (State, BTreeSet<PatchId>, BTreeMap<BlobId, Arc<[u8]>>);

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

    fn writes(&self) -> WriteKey {
        match self {
            Self::InsertRecord { record } | Self::RemoveRecord { record } => {
                WriteKey::RecordLife(record.id)
            }
            Self::ReplaceComponent { record, key, .. } => WriteKey::Component(*record, key.clone()),
        }
    }

    fn accessed_records(&self, records: &mut BTreeSet<RecordId>) {
        match self {
            Self::InsertRecord { record } | Self::RemoveRecord { record } => {
                records.insert(record.id);
                for component in &record.components {
                    records.extend(component.value.record_refs.iter().copied());
                }
            }
            Self::ReplaceComponent {
                record,
                before,
                after,
                ..
            } => {
                records.insert(*record);
                for value in [before, after].into_iter().flatten() {
                    records.extend(value.record_refs.iter().copied());
                }
            }
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum WriteKey {
    RecordLife(RecordId),
    Component(RecordId, ComponentKey),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PatchEffects {
    records: BTreeSet<RecordId>,
    components: BTreeSet<crate::ComponentAddress>,
}

impl PatchEffects {
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

#[derive(Clone, Debug)]
pub(crate) struct PatchSegment {
    pub(crate) id: PatchId,
    pub(crate) requires: BTreeSet<PatchId>,
    pub(crate) operations: Arc<[Operation]>,
    pub(crate) attachments: BTreeMap<BlobId, Arc<[u8]>>,
}

impl PatchSegment {
    pub(crate) fn new(
        base: BaseRevision,
        requires: BTreeSet<PatchId>,
        operations: Vec<Operation>,
        attachments: BTreeMap<BlobId, Arc<[u8]>>,
    ) -> Result<Self> {
        let encoded = revision::to_vec(&SegmentIdentity {
            base,
            requires: requires.iter().copied().collect(),
            operations: operations.clone(),
            attachments: attachments.keys().copied().collect(),
        })?;
        Ok(Self {
            id: PatchId::for_bytes(&encoded),
            requires,
            operations: operations.into(),
            attachments,
        })
    }

    fn writes(&self) -> BTreeSet<WriteKey> {
        self.operations.iter().map(Operation::writes).collect()
    }

    fn accessed_records(&self) -> BTreeSet<RecordId> {
        let mut result = BTreeSet::new();
        for operation in self.operations.iter() {
            operation.accessed_records(&mut result);
        }
        result
    }
}

#[revisioned(revision = 1)]
#[derive(Clone)]
struct SegmentIdentity {
    base: BaseRevision,
    requires: Vec<PatchId>,
    operations: Vec<Operation>,
    attachments: Vec<BlobId>,
}

#[derive(Clone, Debug)]
pub struct Patch {
    base: BaseRevision,
    segments: Arc<[Arc<PatchSegment>]>,
    label: Option<Arc<str>>,
}

impl Patch {
    pub(crate) fn from_segment(base: BaseRevision, segment: PatchSegment) -> Self {
        Self {
            base,
            segments: Arc::from([Arc::new(segment)]),
            label: None,
        }
    }

    pub(crate) fn from_operations(
        base: BaseRevision,
        operations: Vec<Operation>,
        label: Option<Arc<str>>,
    ) -> Result<Self> {
        let segment = PatchSegment::new(base, BTreeSet::new(), operations, BTreeMap::new())?;
        Ok(Self {
            base,
            segments: Arc::from([Arc::new(segment)]),
            label,
        })
    }

    #[must_use]
    pub const fn base(&self) -> BaseRevision {
        self.base
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments
            .iter()
            .all(|segment| segment.operations.is_empty())
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
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.base.document.as_uuid().as_bytes());
        hasher.update(&self.base.revision.get().to_le_bytes());
        hasher.update(&(self.segments.len() as u64).to_le_bytes());
        for segment in self.segments.iter() {
            hasher.update(segment.id.as_bytes());
        }
        if let Some(label) = &self.label {
            hasher.update(&[1]);
            hasher.update(&(label.len() as u64).to_le_bytes());
            hasher.update(label.as_bytes());
        } else {
            hasher.update(&[0]);
        }
        PatchId::for_bytes(hasher.finalize().as_bytes())
    }

    #[must_use]
    pub fn effects(&self) -> PatchEffects {
        let mut effects = PatchEffects::default();
        for operation in self.operations() {
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
            && self.segments.len() == 1
            && self.segments[0].requires == *snapshot.lineage()
    }

    pub fn merge<'a>(patches: impl IntoIterator<Item = &'a Patch>) -> Result<Self> {
        let mut patches = patches.into_iter();
        let first = patches
            .next()
            .ok_or_else(|| Error::invalid("cannot merge an empty patch collection"))?;
        let base = first.base;
        let mut segments = Vec::<Arc<PatchSegment>>::new();
        let mut ids = BTreeSet::new();
        let mut label = first.label.clone();
        for patch in std::iter::once(first).chain(patches) {
            if patch.base != base {
                return Err(Error::patch_conflict("patch bases differ"));
            }
            if label.is_none() {
                label = patch.label.clone();
            }
            for segment in patch.segments.iter() {
                if ids.insert(segment.id) {
                    segments.push(segment.clone());
                }
            }
        }
        validate_segments(&segments, &BTreeSet::new(), true)?;
        Ok(Self {
            base,
            segments: segments.into(),
            label,
        })
    }

    pub(crate) fn apply(&self, state: &State, lineage: &BTreeSet<PatchId>) -> Result<AppliedPatch> {
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
        validate_segments(&self.segments, lineage, false)?;
        let mut next = state.clone();
        let mut next_lineage = lineage.clone();
        let mut attachments = BTreeMap::new();
        for segment in self.segments.iter() {
            if next_lineage.contains(&segment.id) {
                continue;
            }
            for operation in segment.operations.iter() {
                operation.apply(&mut next)?;
            }
            next_lineage.insert(segment.id);
            for (id, bytes) in &segment.attachments {
                if BlobId::for_bytes(bytes) != *id {
                    return Err(Error::invalid("patch attachment hash mismatch"));
                }
                attachments.entry(*id).or_insert_with(|| bytes.clone());
            }
        }
        #[cfg(debug_assertions)]
        next.validate()?;
        Ok((next, next_lineage, attachments))
    }

    pub(crate) fn operations(&self) -> impl Iterator<Item = &Operation> {
        self.segments
            .iter()
            .flat_map(|segment| segment.operations.iter())
    }

    pub(crate) fn attachments(&self) -> impl Iterator<Item = BlobAttachment> + '_ {
        self.segments.iter().flat_map(|segment| {
            segment
                .attachments
                .iter()
                .map(|(id, bytes)| BlobAttachment::from_parts(*id, bytes.clone()))
        })
    }
}

fn validate_segments(
    segments: &[Arc<PatchSegment>],
    initial: &BTreeSet<PatchId>,
    allow_external: bool,
) -> Result<()> {
    let all_ids: BTreeSet<_> = segments.iter().map(|segment| segment.id).collect();
    let mut available = initial.clone();
    for segment in segments {
        for required in &segment.requires {
            if (all_ids.contains(required) || !allow_external) && !available.contains(required) {
                return Err(Error::MissingPatchDependency(*required));
            }
        }
        available.insert(segment.id);
    }

    for (index, left) in segments.iter().enumerate() {
        for right in &segments[index + 1..] {
            if left.requires.contains(&right.id) || right.requires.contains(&left.id) {
                continue;
            }
            let left_writes = left.writes();
            let right_writes = right.writes();
            if let Some(key) = left_writes.intersection(&right_writes).next() {
                return Err(Error::patch_conflict(format!(
                    "unrelated segments write {key:?}"
                )));
            }
            let left_access = left.accessed_records();
            let right_access = right.accessed_records();
            for write in &left_writes {
                if let WriteKey::RecordLife(record) = write
                    && right_access.contains(record)
                {
                    return Err(Error::patch_conflict(format!(
                        "record lifecycle for {record} conflicts with sibling access"
                    )));
                }
            }
            for write in &right_writes {
                if let WriteKey::RecordLife(record) = write
                    && left_access.contains(record)
                {
                    return Err(Error::patch_conflict(format!(
                        "record lifecycle for {record} conflicts with sibling access"
                    )));
                }
            }
        }
    }
    Ok(())
}
