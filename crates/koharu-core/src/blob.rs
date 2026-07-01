//! Content-addressed blob references.
//!
//! `BlobRef` is just the hash. The actual `BlobStore` (filesystem + LRU
//! decode cache) lives in `koharu-app`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Hex-encoded blake3 hash of an immutable blob.
#[derive(
    Default, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, ToSchema,
)]
#[serde(transparent)]
pub struct BlobRef(pub String);

impl BlobRef {
    pub fn new(hash: impl Into<String>) -> Self {
        Self(hash.into())
    }

    pub fn hash(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for BlobRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
