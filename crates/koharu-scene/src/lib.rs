//! Koharu's compact, SQLite-backed manga document model.
//!
//! Reads borrow the committed [`Project`]. Writes are [`Commands`] committed
//! atomically through [`Session`]; [`Edit`] is only fluent syntax over those
//! commands.

mod blob;
mod command;
mod edit;
mod error;
mod geometry;
mod id;
mod model;
mod session;
mod storage;
mod style;

pub use command::{Command, Commands, ElementChange};
pub use edit::{Edit, ImageEdit, PageEdit, TextEdit};
pub use error::{Error, Result};
pub use geometry::{Frame, Quad, Size};
pub use id::{BlobId, ElementId, PageId, ProjectId, Revision};
pub use model::{
    Element, ElementKind, ImageElement, Page, PageAsset, PageAssets, Project, SourceText,
    TextBlock, TextDirection,
};
pub use session::{ChangeSet, GcReport, Options, Session};
pub use style::{
    BevelStyle, BlendMode, Color, FontSlant, GradientStop, StrokePosition, TextAlign,
    TextDecoration, TextEffect, TextEffectKind, TextFit, TextLayout, TextOverflow, TextStyle,
    VerticalAlign, WritingMode,
};

#[cfg(test)]
mod tests;
