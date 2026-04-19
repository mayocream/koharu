use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFontVariant {
    pub style: String,
    pub weight: u16,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFontEntry {
    pub family: String,
    pub category: String,
    pub subsets: Vec<String>,
    pub variants: Vec<GoogleFontVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct GoogleFontCatalog {
    pub fonts: Vec<GoogleFontEntry>,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, JsonSchema, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FontSource {
    System,
    Google,
}
