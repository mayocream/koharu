use std::{fmt, str::FromStr, sync::Arc};

use revision::revisioned;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{BlobId, Error, RecordId, Result};

const MAX_KIND_BYTES: usize = 255;
const MAX_SLOT_BYTES: usize = 127;
const MAX_COMPONENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_REFERENCES: usize = 1_000_000;

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ComponentKind(String);

impl ComponentKind {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.len() > MAX_KIND_BYTES || !value.contains('.') || !valid_name(&value) {
            return Err(Error::invalid("component kind is invalid"));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ComponentKind {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl fmt::Display for ComponentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ComponentSlot(String);

impl ComponentSlot {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let value = if value.is_empty() {
            "default".to_owned()
        } else {
            value
        };
        if value.len() > MAX_SLOT_BYTES || !valid_name(&value) {
            return Err(Error::invalid("component slot is invalid"));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ComponentSlot {
    fn default() -> Self {
        Self("default".to_owned())
    }
}

impl FromStr for ComponentSlot {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl fmt::Display for ComponentSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
pub struct ComponentKey {
    kind: ComponentKind,
    slot: ComponentSlot,
}

impl ComponentKey {
    #[must_use]
    pub const fn new(kind: ComponentKind, slot: ComponentSlot) -> Self {
        Self { kind, slot }
    }

    pub fn named(kind: impl Into<String>, slot: impl Into<String>) -> Result<Self> {
        Ok(Self::new(
            ComponentKind::new(kind)?,
            ComponentSlot::new(slot)?,
        ))
    }

    #[must_use]
    pub const fn kind(&self) -> &ComponentKind {
        &self.kind
    }

    #[must_use]
    pub const fn slot(&self) -> &ComponentSlot {
        &self.slot
    }
}

impl fmt::Display for ComponentKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.kind, self.slot)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentAddress {
    pub record: RecordId,
    pub key: ComponentKey,
}

#[derive(Clone)]
pub struct ComponentRecord {
    schema: u32,
    payload: Arc<[u8]>,
    record_refs: Arc<[RecordId]>,
    blob_refs: Arc<[BlobId]>,
    fingerprint: [u8; 32],
}

impl ComponentRecord {
    pub fn new(
        schema: u32,
        payload: impl Into<Arc<[u8]>>,
        record_refs: impl IntoIterator<Item = RecordId>,
        blob_refs: impl IntoIterator<Item = BlobId>,
    ) -> Result<Self> {
        let payload = payload.into();
        if payload.len() > MAX_COMPONENT_BYTES {
            return Err(Error::invalid("component payload exceeds the hard limit"));
        }
        let mut record_refs = record_refs.into_iter().collect::<Vec<_>>();
        let mut blob_refs = blob_refs.into_iter().collect::<Vec<_>>();
        if record_refs.len() > MAX_REFERENCES || blob_refs.len() > MAX_REFERENCES {
            return Err(Error::invalid(
                "component reference count exceeds the hard limit",
            ));
        }
        record_refs.sort_unstable();
        record_refs.dedup();
        blob_refs.sort_unstable();
        blob_refs.dedup();
        let fingerprint = fingerprint(schema, &payload, &record_refs, &blob_refs);
        Ok(Self {
            schema,
            payload,
            record_refs: Arc::from(record_refs),
            blob_refs: Arc::from(blob_refs),
            fingerprint,
        })
    }

    #[must_use]
    pub const fn schema(&self) -> u32 {
        self.schema
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[must_use]
    pub fn payload_arc(&self) -> Arc<[u8]> {
        self.payload.clone()
    }

    #[must_use]
    pub fn record_refs(&self) -> &[RecordId] {
        &self.record_refs
    }

    #[must_use]
    pub fn blob_refs(&self) -> &[BlobId] {
        &self.blob_refs
    }

    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub(crate) fn to_stored(&self) -> StoredComponent {
        StoredComponent {
            schema: self.schema,
            payload: self.payload.to_vec(),
            record_refs: self.record_refs.to_vec(),
            blob_refs: self.blob_refs.to_vec(),
        }
    }

    pub(crate) fn from_stored(value: StoredComponent) -> Result<Self> {
        if !strictly_sorted(&value.record_refs) || !strictly_sorted(&value.blob_refs) {
            return Err(Error::invalid(
                "stored component references are not canonically sorted",
            ));
        }
        Self::new(
            value.schema,
            value.payload,
            value.record_refs,
            value.blob_refs,
        )
    }
}

fn strictly_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

impl fmt::Debug for ComponentRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ComponentRecord")
            .field("schema", &self.schema)
            .field("payload_bytes", &self.payload.len())
            .field("record_refs", &self.record_refs)
            .field("blob_refs", &self.blob_refs)
            .field("fingerprint", &blake3::Hash::from_bytes(self.fingerprint))
            .finish()
    }
}

impl PartialEq for ComponentRecord {
    fn eq(&self, other: &Self) -> bool {
        self.schema == other.schema
            && self.payload == other.payload
            && self.record_refs == other.record_refs
            && self.blob_refs == other.blob_refs
    }
}

impl Eq for ComponentRecord {}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredComponent {
    pub(crate) schema: u32,
    pub(crate) payload: Vec<u8>,
    pub(crate) record_refs: Vec<RecordId>,
    pub(crate) blob_refs: Vec<BlobId>,
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredComponentEntry {
    pub(crate) key: ComponentKey,
    pub(crate) value: StoredComponent,
}

fn fingerprint(
    schema: u32,
    payload: &[u8],
    record_refs: &[RecordId],
    blob_refs: &[BlobId],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&schema.to_le_bytes());
    hasher.update(&(payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    hasher.update(&(record_refs.len() as u64).to_le_bytes());
    for id in record_refs {
        hasher.update(id.as_uuid().as_bytes());
    }
    hasher.update(&(blob_refs.len() as u64).to_le_bytes());
    for id in blob_refs {
        hasher.update(id.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn valid_name(value: &str) -> bool {
    if value.is_empty() || value.contains('\0') || !value.is_ascii() {
        return false;
    }
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            && segment
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && segment
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
    };
    value.split('.').all(valid_segment)
}
