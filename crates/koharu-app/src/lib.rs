//! Koharu application layer. Holds the top-level `App`, `ProjectSession`,
//! history, blob store, archive I/O, and the engine pipeline.
//!
//! See [`crate::app::App`] for the entry point.

pub mod ai;
pub mod app;
pub mod archive;
pub mod autosave;
pub mod blobs;
pub mod bus;
pub mod config;
pub mod google_fonts;
pub mod history;
pub mod llm;
pub mod pipeline;
pub mod projects;
pub mod renderer;
pub mod session;
pub mod utils;

pub use ai::AiManager;
pub use app::{App, AppSharedState};
pub use blobs::BlobStore;
pub use config::AppConfig;
pub use pipeline::{
    Artifact, Engine, EngineCtx, EngineInfo, PipelineRunOptions, PipelineSpec, Registry, Scope,
};
pub use session::ProjectSession;
