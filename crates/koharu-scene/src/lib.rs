//! Native, SQLite-backed 2D scene graph.
//!
//! [`Scene`] is immutable to callers. All mutations are expressed as a
//! [`CommandBatch`] and committed atomically through [`Session`].

mod blob;
mod command;
mod edit;
mod error;
mod geometry;
mod id;
pub mod node;
mod scene;
mod session;
mod storage;
mod style;

pub use command::{CommandBatch, PagePosition, Parent, Position};
pub use edit::{ContainerEdit, Edit, NodeEdit, PageEdit, TextEdit};
pub use error::{Error, Result};
pub use geometry::{CanvasSize, PixelSize, Transform};
pub use id::{BlobId, CommandId, NodeId, PageId, Revision};
pub use node::{ImageNode, MaskNode, Node, NodeBuilder, NodeKind, TextNode};
pub use scene::{Children, Page, PageRef, Scene, Visit, WalkEvent};
pub use session::{Applied, ChangeSet, GcReport, NodeChangeFlags, Session, SessionConfig};
pub use style::{
    BevelStyle, BevelTechnique, BlendMode, Color, FontSlant, GradientStop, StrokePosition,
    TextAlign, TextDecoration, TextEffect, TextEffectKind, TextLayout, TextOverflow, TextStyle,
    VerticalAlign, WritingMode,
};
