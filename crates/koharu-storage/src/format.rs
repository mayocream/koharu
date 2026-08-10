use std::{
    fs,
    path::{Path, PathBuf},
};

use revision::revisioned;
use serde::{Deserialize, Serialize};

use crate::{BlobId, DocumentId, Error, Result, Revision, durability};

const MAGIC: &[u8; 8] = b"KHRSTATE";
const VERSION: u32 = 1;
const HEADER_BYTES: usize = 8 + 4 + 8 + 32;
const MAX_STATE_BYTES: usize = 512 * 1024 * 1024;

#[revisioned(revision = 1)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StoredState {
    pub(crate) document: DocumentId,
    pub(crate) revision: Revision,
    pub(crate) blobs: Vec<BlobId>,
    pub(crate) payload: Vec<u8>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Slot {
    A,
    B,
}

impl Slot {
    pub(crate) const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::A => "state-a.khr",
            Self::B => "state-b.khr",
        }
    }
}

pub(crate) fn path(root: &Path, slot: Slot) -> PathBuf {
    root.join(slot.filename())
}

pub(crate) fn load(root: &Path, slot: Slot) -> Result<Option<StoredState>> {
    let path = path(root, slot);
    if !path.try_exists()? {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    decode(&bytes).map(Some)
}

pub(crate) fn save(root: &Path, slot: Slot, state: &StoredState) -> Result<()> {
    let bytes = encode(state)?;
    durability::publish(&path(root, slot), &bytes)
}

fn encode(state: &StoredState) -> Result<Vec<u8>> {
    validate(state)?;
    let body = revision::to_vec(state)?;
    if body.len() > MAX_STATE_BYTES {
        return Err(Error::invalid("state exceeds the maximum encoded size"));
    }
    let checksum = blake3::hash(&body);
    let mut encoded = Vec::with_capacity(HEADER_BYTES + body.len());
    encoded.extend_from_slice(MAGIC);
    encoded.extend_from_slice(&VERSION.to_le_bytes());
    encoded.extend_from_slice(&(body.len() as u64).to_le_bytes());
    encoded.extend_from_slice(checksum.as_bytes());
    encoded.extend_from_slice(&body);
    Ok(encoded)
}

fn decode(encoded: &[u8]) -> Result<StoredState> {
    if encoded.len() < HEADER_BYTES || &encoded[..8] != MAGIC {
        return Err(Error::NotAProject);
    }
    let version = u32::from_le_bytes(encoded[8..12].try_into().expect("fixed header range"));
    if version != VERSION {
        return Err(Error::UnsupportedFormat(version));
    }
    let body_len = u64::from_le_bytes(encoded[12..20].try_into().expect("fixed header range"));
    let body_len = usize::try_from(body_len)
        .map_err(|_| Error::invalid("state length does not fit this platform"))?;
    if body_len > MAX_STATE_BYTES || encoded.len() != HEADER_BYTES + body_len {
        return Err(Error::invalid("state length is invalid"));
    }
    let expected = &encoded[20..52];
    let body = &encoded[HEADER_BYTES..];
    if blake3::hash(body).as_bytes() != expected {
        return Err(Error::invalid("state checksum mismatch"));
    }
    let state: StoredState = revision::from_slice(body)?;
    validate(&state)?;
    Ok(state)
}

fn validate(state: &StoredState) -> Result<()> {
    if state.payload.len() > MAX_STATE_BYTES {
        return Err(Error::invalid("scene payload exceeds the maximum size"));
    }
    if state.blobs.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(Error::invalid("state blob IDs are not sorted and unique"));
    }
    Ok(())
}
