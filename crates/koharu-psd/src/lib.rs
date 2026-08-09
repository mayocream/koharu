//! PSD export directly from renderer-resolved page compositions.
//!
//! The binary layout follows GIMP's PSD plug-in at the pinned revision linked from every
//! format implementation. Layer ordering, pixels, visibility, font selection, layout, and the
//! merged preview all come from the same immutable `koharu-renderer` composition.

mod descriptor;
mod document;
mod engine_data;
mod error;
mod export;
mod packbits;
mod writer;

pub use document::{PsdExportOptions, TextLayerMode};
pub use error::PsdExportError;
pub use export::export_page;
