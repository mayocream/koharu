use std::{
    any::Any,
    sync::{Arc, OnceLock},
};

use revision::revisioned;

use crate::{BlobId, EntityId, Error, Result};

pub struct ValidationContext<'a> {
    record_exists: &'a dyn Fn(EntityId) -> bool,
    blob_exists: &'a dyn Fn(BlobId) -> bool,
}

impl<'a> ValidationContext<'a> {
    pub(crate) fn new(
        record_exists: &'a dyn Fn(EntityId) -> bool,
        blob_exists: &'a dyn Fn(BlobId) -> bool,
    ) -> Self {
        Self {
            record_exists,
            blob_exists,
        }
    }

    #[must_use]
    pub fn contains_entity(&self, id: EntityId) -> bool {
        (self.record_exists)(id)
    }

    #[must_use]
    pub fn contains_blob(&self, id: BlobId) -> bool {
        (self.blob_exists)(id)
    }
}

pub trait Component:
    Clone + revision::SerializeRevisioned + revision::DeserializeRevisioned + Send + Sync + 'static
{
    const KIND: &'static str;

    fn record_refs(&self) -> Vec<EntityId> {
        Vec::new()
    }

    fn blob_refs(&self) -> Vec<BlobId> {
        Vec::new()
    }

    fn origin(&self) -> Option<&crate::Origin> {
        None
    }

    fn set_origin(&mut self, _origin: crate::Origin) -> bool {
        false
    }

    fn validate(&self, _context: &ValidationContext<'_>) -> Result<()> {
        Ok(())
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ComponentKey {
    pub(crate) kind: String,
}

impl ComponentKey {
    pub(crate) fn new(kind: impl Into<String>) -> Result<Self> {
        let kind = kind.into();
        if kind.len() > 255 || !kind.contains('.') || !valid_name(&kind, true) {
            return Err(Error::invalid("component key is invalid"));
        }
        Ok(Self { kind })
    }
}

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredComponent {
    pub(crate) schema: u32,
    pub(crate) payload: Vec<u8>,
    pub(crate) record_refs: Vec<EntityId>,
    pub(crate) blob_refs: Vec<BlobId>,
}

#[derive(Clone, Debug)]
pub(crate) struct ComponentRecord {
    pub(crate) schema: u32,
    pub(crate) payload: Arc<[u8]>,
    pub(crate) record_refs: Arc<[EntityId]>,
    pub(crate) blob_refs: Arc<[BlobId]>,
    fingerprint: [u8; 32],
    decoded: Arc<OnceLock<Arc<dyn Any + Send + Sync>>>,
}

impl ComponentRecord {
    pub(crate) fn from_stored(mut stored: StoredComponent) -> Result<Self> {
        stored.record_refs.sort_unstable();
        stored.record_refs.dedup();
        stored.blob_refs.sort_unstable();
        stored.blob_refs.dedup();
        Self::new(
            stored.schema,
            stored.payload,
            stored.record_refs,
            stored.blob_refs,
        )
    }

    pub(crate) fn new(
        schema: u32,
        payload: impl Into<Arc<[u8]>>,
        mut record_refs: Vec<EntityId>,
        mut blob_refs: Vec<BlobId>,
    ) -> Result<Self> {
        let payload = payload.into();
        if payload.len() > 64 * 1024 * 1024 {
            return Err(Error::invalid("component payload exceeds the hard limit"));
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
            decoded: Arc::new(OnceLock::new()),
        })
    }

    pub(crate) fn to_stored(&self) -> StoredComponent {
        StoredComponent {
            schema: self.schema,
            payload: self.payload.to_vec(),
            record_refs: self.record_refs.to_vec(),
            blob_refs: self.blob_refs.to_vec(),
        }
    }

    pub(crate) const fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
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

pub(crate) fn key<T: Component>() -> Result<ComponentKey> {
    ComponentKey::new(T::KIND)
}

pub(crate) fn encode<T: Component>(
    value: &T,
    context: &ValidationContext<'_>,
) -> Result<ComponentRecord> {
    value.validate(context)?;
    let record = ComponentRecord::new(
        u32::from(<T as revision::Revisioned>::revision()),
        revision::to_vec(value)?,
        value.record_refs(),
        value.blob_refs(),
    )?;
    let _ = record.decoded.set(Arc::new(value.clone()));
    Ok(record)
}

pub(crate) fn decode<T: Component>(
    record: &ComponentRecord,
    context: &ValidationContext<'_>,
) -> Result<T> {
    if let Some(value) = record
        .decoded
        .get()
        .and_then(|value| value.downcast_ref::<T>())
    {
        return Ok(value.clone());
    }
    let current = u32::from(<T as revision::Revisioned>::revision());
    if record.schema == 0 || record.schema > current {
        return Err(Error::UnsupportedComponent {
            kind: T::KIND.to_owned(),
            schema: record.schema,
        });
    }
    let value: T = revision::from_slice(&record.payload)?;
    value.validate(context)?;
    let mut expected_records = value.record_refs();
    expected_records.sort_unstable();
    expected_records.dedup();
    let mut expected_blobs = value.blob_refs();
    expected_blobs.sort_unstable();
    expected_blobs.dedup();
    if expected_records.as_slice() != record.record_refs.as_ref()
        || expected_blobs.as_slice() != record.blob_refs.as_ref()
    {
        return Err(Error::ReferenceMismatch(T::KIND.to_owned()));
    }
    let _ = record.decoded.set(Arc::new(value.clone()));
    Ok(value)
}

fn fingerprint(
    schema: u32,
    payload: &[u8],
    record_refs: &[EntityId],
    blob_refs: &[BlobId],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&schema.to_le_bytes());
    hasher.update(payload);
    for id in record_refs {
        hasher.update(id.as_uuid().as_bytes());
    }
    for id in blob_refs {
        hasher.update(id.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn valid_name(value: &str, namespaced: bool) -> bool {
    !value.is_empty()
        && !value.contains('\0')
        && value.is_ascii()
        && (!namespaced || value.contains('.'))
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}
