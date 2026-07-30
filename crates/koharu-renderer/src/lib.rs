//! Scene compilation, Unicode text layout, and reusable WGPU rasterization.
//!
//! Rendering is staged: [`RenderPlan`] resolves semantic scene components,
//! [`PreparedPage`] performs resource decoding and text shaping, and [`Renderer`]
//! owns bounded caches plus the headless raster backend. Prepared pages can also
//! be appended directly to a caller-owned Vello scene for interactive display.

mod bubble;
mod error;
mod font;
mod font_policy;
mod layout;
mod plan;
mod prepare;
mod raster;
mod render;
mod request;
mod resources;
mod script;
mod segment;
mod shape;
mod types;

pub use error::{Error, Result};
pub use font::{Font, FontSystem};
pub use font_policy::FontFallbackPolicy;
pub use layout::{HyphenationPolicy, LayoutLine, LayoutRun, TextLayout, WritingMode};
pub use plan::{RenderBounds, RenderDependency, RenderDiagnostic, RenderPlan};
pub use prepare::{PreparedPage, RenderedEntity, RenderedEntityKind};
pub use raster::{DownsampleFilter, RasterOptions, RenderOptions, StrokeOptions, WgpuRenderer};
pub use render::{RenderOutput, Renderer};
pub use request::{
    BUBBLE_REGION_KIND, RenderRequest, RenderTheme, TEXT_REGION_RELATION_KIND, VerticalAlignment,
};
pub use resources::{FontManager, RenderResources};
pub use segment::{
    LineBreakOpportunity, LineBreakSuffix, LineBreaker, LineSegment, hyphenation_lang_from_tag,
};
pub use shape::{PositionedGlyph, ShapedRun, ShapingOptions, TextShaper};
pub use types::{FontFaceInfo, FontFaceStyle, FontSource, TextAlign};
