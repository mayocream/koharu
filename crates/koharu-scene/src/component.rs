use std::sync::Arc;

use crate::{BlobId, ComponentSlot, EntityId, Error, Result};

#[derive(Clone, Debug)]
pub struct EncodedSceneComponent {
    schema: u32,
    payload: Arc<[u8]>,
}

impl EncodedSceneComponent {
    #[must_use]
    pub fn new(schema: u32, payload: impl Into<Arc<[u8]>>) -> Self {
        Self {
            schema,
            payload: payload.into(),
        }
    }

    #[must_use]
    pub const fn schema(&self) -> u32 {
        self.schema
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

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

pub trait SceneComponent: Clone + Sized + Send + Sync + 'static {
    const KIND: &'static str;
    const CURRENT_SCHEMA: u32;

    fn encode(&self) -> Result<EncodedSceneComponent>;
    fn decode(schema: u32, payload: &[u8]) -> Result<Self>;

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

pub(crate) fn key<T: SceneComponent>(
    slot: impl Into<ComponentSlot>,
) -> Result<koharu_storage::ComponentKey> {
    let slot = slot.into();
    Ok(koharu_storage::ComponentKey::new(
        koharu_storage::ComponentKind::new(T::KIND)?,
        slot.storage()?,
    ))
}

pub(crate) fn encode<T: SceneComponent>(
    value: &T,
    context: &ValidationContext<'_>,
) -> Result<koharu_storage::ComponentRecord> {
    value.validate(context)?;
    let encoded = value.encode()?;
    if encoded.schema() != T::CURRENT_SCHEMA {
        return Err(Error::invalid(format!(
            "{} encoded schema {}, expected {}",
            T::KIND,
            encoded.schema(),
            T::CURRENT_SCHEMA
        )));
    }
    koharu_storage::ComponentRecord::new(
        encoded.schema(),
        Arc::<[u8]>::from(encoded.payload()),
        value.record_refs().into_iter().map(EntityId::storage),
        value.blob_refs(),
    )
    .map_err(Into::into)
}

pub(crate) fn decode<T: SceneComponent>(
    slot: &str,
    record: &koharu_storage::ComponentRecord,
    context: &ValidationContext<'_>,
) -> Result<T> {
    let value = T::decode(record.schema(), record.payload())?;
    value.validate(context)?;
    let mut expected_records = value
        .record_refs()
        .into_iter()
        .map(EntityId::storage)
        .collect::<Vec<_>>();
    expected_records.sort_unstable();
    expected_records.dedup();
    let mut expected_blobs = value.blob_refs();
    expected_blobs.sort_unstable();
    expected_blobs.dedup();
    if expected_records != record.record_refs() || expected_blobs != record.blob_refs() {
        return Err(Error::ReferenceMismatch(format!("{}@{slot}", T::KIND)));
    }
    Ok(value)
}

pub(crate) fn revision_encode<T: revision::SerializeRevisioned>(
    value: &T,
    schema: u32,
) -> Result<EncodedSceneComponent> {
    Ok(EncodedSceneComponent::new(schema, revision::to_vec(value)?))
}

pub(crate) fn revision_decode<T: revision::DeserializeRevisioned>(
    kind: &str,
    schema: u32,
    current: u32,
    payload: &[u8],
) -> Result<T> {
    if schema > current || schema == 0 {
        return Err(Error::UnsupportedComponent {
            kind: kind.to_owned(),
            slot: "unknown".to_owned(),
            schema,
        });
    }
    revision::from_slice(payload).map_err(Into::into)
}
