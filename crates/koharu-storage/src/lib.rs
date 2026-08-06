//! RocksDB-backed project storage.
//!
//! A project directory is the database. Document state, ordered history, and
//! content-addressed blobs occupy separate column families; only the blob
//! column family uses RocksDB's integrated BlobDB.

mod blob;
mod commit;
mod database;
mod error;
mod ids;
mod session;
mod snapshot;

pub use blob::{Blob, BlobBatch};
pub use commit::{Commit, HistoryEntry, Recovery, Refresh};
pub use error::{Error, Result};
pub use ids::{BlobId, DocumentId, PatchId, Revision};
pub use session::{GcReport, Options, Session};
pub use snapshot::Snapshot;

#[cfg(test)]
mod tests;
