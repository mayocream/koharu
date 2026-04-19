//! Self-contained primitive types for ML detection/recognition outputs.
//!
//! Detector/OCR/font-prediction modules return these; the app layer
//! (`koharu-app`) maps them into scene `TextData` / `Op` values.

use serde::{Deserialize, Serialize};

/// Reading axis of a detected text region.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextDirection {
    #[default]
    Horizontal,
    Vertical,
}

/// A four-point polygon, ordered clockwise from the top-left.
pub type Quad = [[f32; 2]; 4];

/// A rectangle-ish detected text region with geometry + detector metadata.
/// This is what detectors emit; the app layer maps it into a scene `TextData` node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
    pub line_polygons: Option<Vec<Quad>>,
    pub source_direction: Option<TextDirection>,
    pub rotation_deg: Option<f32>,
    pub detected_font_size_px: Option<f32>,
    pub detector: Option<String>,
}

/// Font-prediction output from a font-detection model.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedFontPrediction {
    pub index: usize,
    pub name: String,
    pub language: Option<String>,
    pub probability: f32,
    pub serif: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopFont {
    pub index: usize,
    pub score: f32,
}
