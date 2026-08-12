//! Koharu application ownership independent of its desktop shell.

mod agent;
mod application;
mod dialogs;
mod event_hub;
mod presentation;
mod presentation_coordinator;
mod processing;
mod project;

pub use application::{Application, BinaryAttachmentPayload, DispatchOutput, Lifecycle};
pub use dialogs::{DialogFilter, FileDialogs};
pub use event_hub::EventHub;
pub use presentation::{
    CanvasOperation, CanvasOutput, InpaintCommit, PageRenderer, Presentation, PresentationUpdate,
    RasterStroke, TransformCommit, ViewDisposition,
};
pub use processing::{KoharuProcessingRuntime, ProcessingRuntime};
pub use project::ProjectLibrary;
