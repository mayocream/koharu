use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum TextShaderEffect {
    #[default]
    Normal,
    Antique,
    Metal,
    Manga,
    MotionBlur,
}

impl TextShaderEffect {
    pub fn id(self) -> f32 {
        match self {
            Self::Normal => 0.0,
            Self::Antique => 1.0,
            Self::Metal => 2.0,
            Self::Manga => 3.0,
            Self::MotionBlur => 4.0,
        }
    }
}

impl fmt::Display for TextShaderEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Normal => "normal",
            Self::Antique => "antique",
            Self::Metal => "metal",
            Self::Manga => "manga",
            Self::MotionBlur => "motionblur",
        };
        f.write_str(value)
    }
}

impl FromStr for TextShaderEffect {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let effect = match s.trim().to_lowercase().as_str() {
            "normal" => Self::Normal,
            "antique" => Self::Antique,
            "metal" => Self::Metal,
            "manga" => Self::Manga,
            "motionblur" | "motion_blur" => Self::MotionBlur,
            _ => anyhow::bail!(
                "Unknown shader effect: {s}. Valid: normal, antique, metal, manga, motionblur"
            ),
        };
        Ok(effect)
    }
}
