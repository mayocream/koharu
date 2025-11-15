//! Font management utilities for the renderer.
//!
//! This crate currently focuses on providing a simple system font provider
//! that loads fonts from the operating system via `fontdb` and exposes them
//! through `swash`'s `FontRef`.

pub mod font;
pub mod layout;
pub mod render;

pub use font::{Font, FontBook};
pub use fontdb::{FaceInfo, Query};
pub use layout::{
    LayoutBounds, LayoutLine, LayoutRequest, Orientation, LayoutResult, LayoutSession,
    TextLayouter,
};
pub use render::{RenderRequest, RenderedText, TextRenderer};
pub use swash::shape::cluster::Glyph;
