//! Scene-facing font-prediction types.
//!
//! These mirror the raw predictions emitted by `koharu-ml`'s font detector but
//! add the OpenAPI/JSON-Schema derives so they can live on `TextData.font_prediction`
//! and appear in the typed HTTP surface.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Reading axis of a text block.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TextDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopFont {
    pub index: usize,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NamedFontPrediction {
    pub index: usize,
    pub name: String,
    pub language: Option<String>,
    pub probability: f32,
    pub serif: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FontPrediction {
    pub top_fonts: Vec<TopFont>,
    pub named_fonts: Vec<NamedFontPrediction>,
    pub direction: TextDirection,
    pub text_color: [u8; 3],
    pub stroke_color: [u8; 3],
    pub font_size_px: f32,
    pub stroke_width_px: f32,
    pub line_height: f32,
    pub angle_deg: f32,
}

impl Default for FontPrediction {
    fn default() -> Self {
        Self {
            top_fonts: Vec::new(),
            named_fonts: Vec::new(),
            direction: TextDirection::Horizontal,
            text_color: [0, 0, 0],
            stroke_color: [0, 0, 0],
            font_size_px: 0.0,
            stroke_width_px: 0.0,
            line_height: 1.0,
            angle_deg: 0.0,
        }
    }
}
