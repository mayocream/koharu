//! Text layout, font matching, bubble-aware placement, and WGPU rasterization.

mod bubble;
mod compositor;
mod font;
mod layout;
mod renderer;
mod script;
mod segment;
mod shape;
mod types;

pub use bubble::{BubbleIndex, BubbleMatch, LayoutBox};
pub use compositor::{PageRenderOptions, RenderedElement, RenderedPage, Renderer, SceneRenderer};
pub use font::{Font, FontSystem};
pub use layout::{LayoutLine, LayoutRun, TextLayout, WritingMode};
pub use renderer::{DownsampleFilter, RasterOptions, RenderOptions, StrokeOptions, WgpuRenderer};
pub use segment::{
    LineBreakOpportunity, LineBreakSuffix, LineBreaker, LineSegment, hyphenation_lang_from_tag,
};
pub use shape::{PositionedGlyph, ShapedRun, ShapingOptions, TextShaper};
pub use types::{FontFaceInfo, FontSource, TextAlign};
