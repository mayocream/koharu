//! Koharu's persisted scene-graph engine.
//!
//! The crate owns the graph model, reversible operations, durable history,
//! content-addressed lossless WebP blobs, on-disk project sessions, and
//! maximum-compression ZIP archives. Application workflows and transport DTOs
//! live in `koharu-app` and `koharu-rpc`.

pub mod archive;
pub mod blob;
pub mod font;
pub mod history;
pub mod operation;
pub mod project;
pub mod scene;
pub mod style;

pub use archive::{export_khr, export_khr_bytes, import_khr, import_khr_bytes};

pub use blob::{BlobRef, BlobStore};
pub use font::{FontPrediction, NamedFontPrediction, TextDirection, TopFont};
pub use history::History;
pub use operation::{
    ImageDataPatch, MaskDataPatch, NodeDataPatch, NodePatch, Op, OpError, OpResult, PagePatch,
    ProjectMetaPatch, TextDataPatch,
};
pub use project::ProjectSession;
pub use scene::{
    ImageData, ImageRole, MaskData, MaskRole, Node, NodeId, NodeKind, NodeKindTag, Page, PageId,
    ProjectMeta, ProjectStyle, Scene, TextData, Transform,
};
pub use style::{TextAlign, TextShaderEffect, TextStrokeStyle, TextStyle};
