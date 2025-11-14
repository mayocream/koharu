//! Font management utilities for the renderer.
//!
//! This crate currently focuses on providing a simple system font provider
//! that loads fonts from the operating system via `fontdb` and exposes them
//! through `swash`'s `FontRef`.

pub mod font;

pub use font::{Font, FontBook, FontMetadata, FontQuery};
