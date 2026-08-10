//! Filesystem-native project snapshots and immutable content-addressed blobs.

mod blobs;
mod durability;
mod error;
mod format;
mod ids;
mod session;

pub use blobs::Blobs;
pub use bytes::Bytes;
pub use error::{Error, Result};
pub use ids::{BlobId, DocumentId, PatchId, Revision};
pub use session::{GcReport, Session, State};

#[cfg(test)]
mod tests;
