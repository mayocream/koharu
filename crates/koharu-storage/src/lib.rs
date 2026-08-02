//! Durable single-file storage for Koharu documents.
//!
//! Storage owns revision ordering, opaque commits and checkpoints,
//! content-addressed blobs, redb transactions, and durability. It deliberately
//! does not interpret a document's in-memory model. Scene semantics, indexes,
//! validation, conflict detection, and undo operations belong to
//! `koharu-scene`.

mod blob;
mod error;
mod history;
mod id;
mod session;
mod snapshot;
mod storage;

pub use blob::{BlobAttachment, BlobBatch};
pub use error::{Error, Result};
pub use history::{Commit, CommitRequest, Recovery, Refresh};
pub use id::{BlobId, DocumentId, PatchId, Revision};
pub use session::{CommitResult, GcReport, Options, Session};
pub use snapshot::Snapshot;

#[cfg(test)]
mod tests;
