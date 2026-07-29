use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Type,
    strum::Display,
    strum::EnumIter,
    strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum Stage {
    Detection,
    Ocr,
    Translation,
    Inpainting,
}

impl Stage {
    pub const ALL: [Self; 4] = [
        Self::Detection,
        Self::Ocr,
        Self::Translation,
        Self::Inpainting,
    ];
}
