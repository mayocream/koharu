use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TextDirection {
    Horizontal,
    Vertical,
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
pub struct FontPrediction {
    pub top_fonts: Vec<(usize, f32)>,
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
