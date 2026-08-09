//! Retained, performance-first rendering for authored Koharu pages.
//!
//! [`Renderer`] is the only stateful owner. It asynchronously resolves resources,
//! shapes text, retains local vector nodes, and rasterizes immutable [`Composition`]s.

mod bubble;
mod error;
mod fonts;
mod layout;
mod rasterizer;
mod renderer;
mod script;
mod segment;
mod shape;
mod text_renderer;
mod types;

pub use error::{Error, Result};
pub use rasterizer::{DownsampleFilter, RasterImage, RasterOptions};
pub use renderer::{
    Composition, CompositionStats, Layer, LayerKind, PixelMetadata, Presentation, RenderBounds,
    RenderDiagnostic, Renderer, TextMetadata,
};
pub use types::{FontFace, FontFamily, FontMetadata, FontRange, FontSource, FontStyle};

pub(crate) use layout::{HyphenationPolicy, LayoutRun, TextLayout, WritingMode};
pub(crate) use types::TextAlign;
