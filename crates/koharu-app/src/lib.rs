//! Koharu application layer. Holds top-level workflows, state, shared DTOs,
//! archive I/O, and the engine pipeline.
//!
//! See [`crate::app::App`] for the entry point.

pub mod ai;
pub mod app;
pub mod archive;
pub mod autosave;
pub mod bus;
pub mod config;
pub mod events;
pub mod llm;
pub mod pipeline;
pub mod projects;
pub mod protocol;
pub mod renderer;
pub mod utils;

pub use ai::AiManager;
pub use app::{App, AppSharedState};
pub use config::AppConfig;
pub use events::{
    AppEvent, DownloadProgress, DownloadStatus, JobFinishedEvent, JobStatus, JobSummary,
    JobWarningEvent, PipelineProgress, PipelineStatus, PipelineStep, ProjectSummary, SnapshotEvent,
};
pub use pipeline::{
    Artifact, Engine, EngineCtx, EngineInfo, PipelineRunOptions, PipelineSpec, Registry, Scope,
};
pub use protocol::{
    ConfigPatch, DataConfigPatch, EngineCatalog, EngineCatalogEntry, FontFaceInfo, FontSource,
    HttpConfigPatch, LlmCatalog, LlmCatalogModel, LlmGenerationOptions, LlmLoadRequest,
    LlmProviderCatalog, LlmProviderCatalogStatus, LlmState, LlmStateStatus, LlmTarget,
    LlmTargetKind, MetaInfo, PipelineConfigPatch, PipelineLlmRequest, ProviderPatch, ReadingOrder,
    Region,
};
