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
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

impl From<koharu_scene::FontStyle> for FontStyle {
    fn from(value: koharu_scene::FontStyle) -> Self {
        match value {
            koharu_scene::FontStyle::Normal => Self::Normal,
            koharu_scene::FontStyle::Italic => Self::Italic,
            koharu_scene::FontStyle::Oblique => Self::Oblique,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FontFamily {
    pub name: String,
    pub metadata: FontMetadata,
    pub sources: Vec<FontSource>,
    pub faces: Vec<FontFace>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FontMetadata {
    pub primary_script: Option<String>,
    pub scripts: Vec<String>,
    pub languages: Vec<String>,
    pub category: Option<String>,
    pub classifications: Vec<String>,
    pub use_cases: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FontRange {
    pub minimum: u16,
    pub maximum: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FontFace {
    pub post_script_name: String,
    pub weight: u16,
    pub weight_range: Option<FontRange>,
    pub style: FontStyle,
}

/// Inline-axis alignment within a text layout box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}
