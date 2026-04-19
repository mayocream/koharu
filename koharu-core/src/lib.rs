//! Pure scene / op / protocol types. No I/O, no tokio, no HTTP.
//!
//! - `scene` — project, pages, nodes, image/text/mask data, transforms.
//! - `op` — the `Op` enum (the only way the scene changes) + patches + apply/inverse.
//! - `blob` — `BlobRef` (content hash). The actual store lives in `koharu-app`.
//! - `events` — `AppEvent` and its component progress types.
//! - `protocol` — non-scene DTOs: meta, LLM lifecycle, engine catalog, config.
//! - `style`, `font`, `google_fonts` — text styling + font-prediction types.

pub mod blob;
pub mod events;
pub mod font;
pub mod google_fonts;
pub mod op;
pub mod protocol;
pub mod scene;
pub mod style;

pub use blob::BlobRef;
pub use events::{
    AppEvent, DownloadProgress, DownloadStatus, JobFinishedEvent, JobStatus, JobSummary,
    PipelineProgress, PipelineStatus, PipelineStep, ProjectSummary, SnapshotEvent,
};
pub use font::{FontPrediction, NamedFontPrediction, TextDirection, TopFont};
pub use google_fonts::{FontSource, GoogleFontCatalog, GoogleFontEntry, GoogleFontVariant};
pub use op::{
    ImageDataPatch, MaskDataPatch, NodeDataPatch, NodePatch, Op, OpError, OpResult, PagePatch,
    ProjectMetaPatch, TextDataPatch,
};
pub use protocol::{
    ConfigPatch, DataConfigPatch, EngineCatalog, EngineCatalogEntry, FontFaceInfo, HttpConfigPatch,
    LlmCatalog, LlmCatalogModel, LlmGenerationOptions, LlmLoadRequest, LlmProviderCatalog,
    LlmProviderCatalogStatus, LlmState, LlmStateStatus, LlmTarget, LlmTargetKind, MetaInfo,
    PipelineConfigPatch, PipelineLlmRequest, ProviderPatch, Region,
};
pub use scene::{
    ImageData, ImageRole, MaskData, MaskRole, Node, NodeId, NodeKind, NodeKindTag, Page, PageId,
    ProjectMeta, ProjectStyle, Scene, TextData, Transform,
};
pub use style::{TextAlign, TextShaderEffect, TextStrokeStyle, TextStyle};
