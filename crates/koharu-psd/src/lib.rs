//! PSD export directly from Koharu scene revisions and renderer-resolved presentation.
//!
//! The binary layout follows GIMP's PSD plug-in at the pinned revision linked from every
//! format implementation. Scene semantics remain owned by `koharu-scene`; font selection,
//! layout, raster fallback, and the merged preview remain owned by `koharu-renderer`.

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
