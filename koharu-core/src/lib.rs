pub mod commands;
pub mod events;
pub mod google_fonts;
pub mod parse;
pub mod protocol;
pub mod views;

mod effect;
mod font;
mod image;

pub use commands::*;
pub use effect::TextShaderEffect;
pub use events::*;
pub use font::{FontPrediction, NamedFontPrediction, TextDirection, TopFont};
pub use google_fonts::{FontSource, GoogleFontCatalog, GoogleFontEntry, GoogleFontVariant};
pub use image::SerializableDynamicImage;
pub use protocol::*;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// A content-addressable reference to a blob (blake3 hash hex string).
#[derive(Default, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlobRef(pub String);

impl BlobRef {
    pub fn new(hash: impl Into<String>) -> Self {
        Self(hash.into())
    }
    pub fn hash(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for BlobRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn new_text_block_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextBlock {
    #[serde(default = "new_text_block_id")]
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
    pub line_polygons: Option<Vec<[[f32; 2]; 4]>>,
    pub source_direction: Option<TextDirection>,
    pub rendered_direction: Option<TextDirection>,
    pub source_language: Option<String>,
    pub rotation_deg: Option<f32>,
    pub detected_font_size_px: Option<f32>,
    pub detector: Option<String>,
    pub text: Option<String>,
    pub translation: Option<String>,
    pub style: Option<TextStyle>,
    pub font_prediction: Option<FontPrediction>,
    pub rendered: Option<BlobRef>,
    #[serde(default)]
    pub lock_layout_box: bool,
    /// Actual render area — set by renderer when bubble expansion is used.
    /// Frontend and composite use these for sprite positioning when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_x: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_y: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_height: Option<f32>,
}

impl Default for TextBlock {
    fn default() -> Self {
        Self {
            id: new_text_block_id(),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            confidence: 0.0,
            line_polygons: None,
            source_direction: None,
            rendered_direction: None,
            source_language: None,
            rotation_deg: None,
            detected_font_size_px: None,
            detector: None,
            text: None,
            translation: None,
            style: None,
            font_prediction: None,
            rendered: None,
            lock_layout_box: false,
            render_x: None,
            render_y: None,
            render_width: None,
            render_height: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextStrokeStyle {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_stroke_color")]
    pub color: [u8; 4],
    #[serde(default)]
    pub width_px: Option<f32>,
}

impl Default for TextStrokeStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [255, 255, 255, 255],
            width_px: None,
        }
    }
}

const fn default_true() -> bool {
    true
}

const fn default_stroke_color() -> [u8; 4] {
    [255, 255, 255, 255]
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ToSchema, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    pub font_families: Vec<String>,
    pub font_size: Option<f32>,
    pub color: [u8; 4],
    pub effect: Option<TextShaderEffect>,
    pub stroke: Option<TextStrokeStyle>,
    #[serde(default)]
    pub text_align: Option<TextAlign>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BubbleRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_font: Option<String>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub source: BlobRef,
    pub segment: Option<BlobRef>,
    pub inpainted: Option<BlobRef>,
    pub rendered: Option<BlobRef>,
    pub brush_layer: Option<BlobRef>,
    pub text_blocks: Vec<TextBlock>,
    #[serde(default)]
    pub bubbles: Vec<BubbleRegion>,
    #[serde(default)]
    pub style: Option<DocumentStyle>,
}
#[cfg(test)]
mod tests {}
