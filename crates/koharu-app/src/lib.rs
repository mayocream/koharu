//! Headless application state and the desktop protocol.
//!
//! This crate deliberately has no dependency on Winit, Wry, WGPU, native
//! dialogs, or the Koharu desktop shell. Native adapters belong in `koharu`.

mod project;
pub mod protocol;

pub mod app;
mod jobs;
mod resources;

pub use project::{Project, classify_error, failure, project_name};

pub async fn serve_worker() -> anyhow::Result<()> {
    koharu_pipeline::serve_worker().await
}
