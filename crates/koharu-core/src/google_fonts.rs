use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, JsonSchema, ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FontSource {
    System,
    Google,
}
