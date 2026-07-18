//! Shared public value types for renderer entry points.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FontSource {
    System,
    Google,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FontFaceInfo {
    pub family_name: String,
    pub post_script_name: String,
    pub source: FontSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub cached: bool,
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
