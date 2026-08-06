//! Scene composition, Unicode text rendering, and reusable Vello rasterization.
//!
//! Rendering is staged: [`Compositor::compile`] produces a [`Composition`],
//! [`SceneRenderer::render`] records a retained [`Frame`], and
//! [`Rasterizer::rasterize`] turns that frame into pixels. [`TextRenderer`]
//! owns the layout and glyph-recording boundary used by the scene renderer.

mod bubble;
mod compositor;
mod error;
mod fonts;
mod layout;
mod rasterizer;
mod request;
mod scene_renderer;
mod script;
mod segment;
mod shape;
mod text_renderer;
mod types;

pub use compositor::{Composition, Compositor, RenderBounds, RenderDependency, RenderDiagnostic};
pub use error::{Error, Result};
pub use fonts::{Font, FontSystem};
pub use layout::{HyphenationPolicy, LayoutLine, LayoutRun, TextLayout, WritingMode};
pub use rasterizer::{DownsampleFilter, Raster, RasterOptions, Rasterizer};
pub use request::{LayerPresentation, RenderRequest, RenderTheme, VerticalAlignment};
pub use scene_renderer::{
    FontPreview, Frame, SceneRenderer, VisualLayer, VisualLayerKind, VisualText,
};
pub use segment::{
    LineBreakOpportunity, LineBreakSuffix, LineBreaker, LineSegment, hyphenation_lang_from_tag,
};
pub use shape::{PositionedGlyph, ShapedRun, ShapingOptions, TextShaper};
pub use text_renderer::{StrokeOptions, TextRenderOptions, TextRenderer};
pub use types::{FontFaceInfo, FontFaceStyle, FontSource, TextAlign};
