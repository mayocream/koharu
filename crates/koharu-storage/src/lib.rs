//! Generic, versioned record/component storage for Koharu.
//!
//! Storage owns identities, opaque component envelopes, immutable snapshots,
//! flat optimistic patches, explicit read observations, content-addressed
//! bytes, SQLite durability, and revision history. Domain schemas and typed
//! codecs belong in crates such as `koharu-scene`.

mod blob;
mod component;
mod edit;
mod error;
mod history;
mod id;
mod patch;
mod session;
mod snapshot;
mod state;
mod storage;

pub use blob::{BlobAttachment, BlobBatch};
pub use component::{
    ComponentAddress, ComponentKey, ComponentKind, ComponentRecord, ComponentSlot,
};
pub use edit::{Edit, EditView};
pub use error::{Error, Result};
pub use history::{ChangeSet, ComponentChange, RecordChange, ValueChangeKind};
pub use id::{BlobId, DocumentId, PatchId, RecordId, Revision};
pub use patch::{BaseRevision, Patch, PatchEffects};
pub use session::{CommitResult, GcReport, Options, Session};
pub use snapshot::Snapshot;
pub use state::RecordRef;

#[cfg(test)]
mod tests;
