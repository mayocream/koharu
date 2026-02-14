use serde::{Deserialize, Serialize};

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
