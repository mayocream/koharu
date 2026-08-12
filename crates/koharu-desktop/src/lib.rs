//! Single-presenter desktop shell for Koharu.
//!
//! `Presenter` is the only owner of the WGPU surface. The canvas and
//! windowless browser render into textures which the presenter composites into
//! that surface. CEF's browser process shares the winit event thread while its
//! renderer/helper processes remain Chromium-managed subprocesses.

pub mod browser;
mod compositor;
mod damage;
pub mod geometry;
pub mod platform;
mod presenter;
pub mod runtime;

pub use presenter::{PresentOutcome, Presenter, PresenterError};
