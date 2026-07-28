//! Typed, evolvable scene semantics over [`koharu_storage`].
//!
//! The storage crate remains ignorant of pages, hierarchy, relations, text,
//! assets, and model provenance. This crate supplies those meanings through
//! independently versioned components and validates them at every scene
//! snapshot boundary.

mod change;
mod component;
mod components;
mod edit;
mod error;
mod id;
mod index;
mod patch;
mod session;
mod snapshot;

pub use change::{ComponentChange, EntityChange, RelationChange, SceneChangeSet};
pub use component::{EncodedSceneComponent, SceneComponent, ValidationContext};
pub use components::{
    Asset, AssetInput, AssetMetadata, AssetRole, Authored, Children, DetectionAnalysis,
    DetectionLabel, EntityOrigin, Generation, Geometry, LanguageTag, OcrAnalysis, Origin, Page,
    PageDraft, Point, ProjectSettings, ReadingOrder, Region, RegionKind, Relation, RelationKind,
    SourceText, TextAlignment, TextDirection, TextRole, Translation, Typography, Visibility,
    WritingMode,
};
pub use edit::{At, RemovePolicy, SceneEdit};
pub use error::{Error, Result};
pub use id::{ComponentSlot, EntityId, ProducerId, ProjectId, RelationId};
pub use patch::ScenePatch;
pub use session::{SceneCommit, SceneSession};
pub use snapshot::{EntityRef, PageRef, RelationRef, SceneSnapshot};

pub use koharu_storage::{BlobBatch, BlobId, PatchId, Revision};

#[cfg(test)]
mod tests;
