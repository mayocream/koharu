//! Koharu's scene-backed application layer and desktop protocol.
//!
//! [`Project`] and [`protocol`] are platform-independent. The [`app`] module is
//! the native adapter that coordinates the desktop, canvas, renderer, and
//! in-process pipeline without creating a second durable document model.

mod project;
mod projection;
pub mod protocol;

pub mod app;
mod jobs;
mod resources;

pub use project::{Project, classify_error, failure, project_name};
