//! Shared public value types for renderer entry points.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FontSource {
    System,
    Bundled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FontFaceStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FontFamilyInfo {
    pub family_name: String,
    pub primary_script: Option<String>,
    pub scripts: Vec<String>,
    pub primary_language: Option<String>,
    pub languages: Vec<String>,
    pub faces: Vec<FontFaceInfo>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FontAxisRange {
    pub minimum: u16,
    pub maximum: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FontFaceInfo {
    pub font_name: String,
    pub post_script_name: String,
    pub weight: u16,
    pub weight_range: Option<FontAxisRange>,
    pub stretch: u16,
    pub stretch_range: Option<FontAxisRange>,
    pub style: FontFaceStyle,
    pub source: FontSource,
}

/// Horizontal alignment within a text layout box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}
