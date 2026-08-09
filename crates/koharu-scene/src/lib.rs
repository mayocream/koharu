//! Typed, evolvable Koharu project semantics over [`koharu_storage`].
//!
//! The storage crate remains ignorant of pages, hierarchy, relations, text,
//! assets, and model provenance. This crate supplies those meanings through
//! independently versioned components and validates them at every project
//! snapshot boundary.

mod change;
mod component;
mod components;
mod document;
mod edit;
mod error;
mod id;
mod patch;
mod schema;
mod semantics;
mod session;
mod snapshot;
mod state;

pub use change::{
    Change, ComponentChange, ComponentOwner, EntityChange, RelationChange, ValueChangeKind,
};
pub use component::{Component, ValidationContext};
pub use components::{
    Asset, AssetInput, AssetMetadata, AssetRef, AssetRole, Authored, DetectionAnalysis,
    DetectionLabel, EntityOrigin, FontStyle, Generation, Geometry, Group, LanguageTag, MaskChannel,
    OcrAnalysis, Origin, Page, PageDraft, PixelFormat, PixelLayer, Point, Project, Region,
    RegionKind, Relation, RelationKind, SourceText, TextAlignment, TextContent, TextDirection,
    TextGroup, TextLayout, TextLayoutKind, TextRole, TextStroke, Translation, Typography,
    VerticalAlignment, Visibility, WritingMode,
};
pub use document::{AnalysisRegionRef, GroupRef, PixelLayerRef, TextContentRef, TextLayerRef};
pub use edit::{At, Edit, RemovePolicy};
pub use error::{Error, Result};
pub use id::{EntityId, ProducerId, ProjectId, RelationId};
pub use patch::Patch;
pub use semantics::{
    BubbleRegion, FitsTo, FunctionalRelation, Inside, PanelRegion, Presents, RecognizedFrom,
    RegionSpec, RelationSpec, TextRegion,
};
pub use session::{Commit, Session};
pub use snapshot::{EntityRef, PageRef, RelationRef, Snapshot};

pub use koharu_storage::{BlobBatch, BlobId, PatchId, Revision};

#[cfg(test)]
mod tests;
