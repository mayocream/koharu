//! Text styling types shared by scene nodes, the API, and the renderer.

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use strum::IntoEnumIterator;
use utoipa::ToSchema;

// ---------------------------------------------------------------------------
// Alignment
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ToSchema, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

// ---------------------------------------------------------------------------
// Shader effect (italic / bold flags)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumIter, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
enum TextShaderEffectFlag {
    Italic,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextShaderEffect {
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub bold: bool,
}

impl TextShaderEffect {
    pub const ITALIC_FLAG: u32 = 1 << 0;
    pub const BOLD_FLAG: u32 = 1 << 1;

    pub fn flags(self) -> u32 {
        let mut flags = 0u32;
        if self.italic {
            flags |= Self::ITALIC_FLAG;
        }
        if self.bold {
            flags |= Self::BOLD_FLAG;
        }
        flags
    }

    pub fn is_empty(self) -> bool {
        self.flags() == 0
    }

    pub fn none() -> Self {
        Self {
            italic: false,
            bold: false,
        }
    }

    fn apply_flag(&mut self, flag: TextShaderEffectFlag) {
        match flag {
            TextShaderEffectFlag::Italic => self.italic = true,
            TextShaderEffectFlag::Bold => self.bold = true,
        }
    }

    fn enabled_flags(self) -> [Option<TextShaderEffectFlag>; 2] {
        [
            self.italic.then_some(TextShaderEffectFlag::Italic),
            self.bold.then_some(TextShaderEffectFlag::Bold),
        ]
    }
}

fn valid_shader_effects() -> String {
    TextShaderEffectFlag::iter()
        .map(|flag| flag.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for TextShaderEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = self
            .enabled_flags()
            .into_iter()
            .flatten()
            .map(|flag| flag.to_string())
            .collect::<Vec<_>>();

        if parts.is_empty() {
            f.write_str("none")
        } else {
            f.write_str(&parts.join(","))
        }
    }
}

impl FromStr for TextShaderEffect {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_lowercase();
        if normalized.is_empty() || normalized == "none" || normalized == "normal" {
            return Ok(Self::none());
        }

        let mut effect = Self::none();
        for token in normalized
            .split(|c: char| c == ',' || c == '|' || c == '+' || c.is_whitespace())
            .filter(|token| !token.is_empty())
        {
            if matches!(token, "normal" | "none") {
                continue;
            }

            let flag = token.parse::<TextShaderEffectFlag>().map_err(|_| {
                anyhow::anyhow!(
                    "Unknown shader effect: {token}. Valid: {}",
                    valid_shader_effects()
                )
            })?;
            effect.apply_flag(flag);
        }

        Ok(effect)
    }
}

impl<'de> Deserialize<'de> for TextShaderEffect {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct FlagsRepr {
            italic: Option<bool>,
            bold: Option<bool>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Flags(FlagsRepr),
            Legacy(String),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Flags(FlagsRepr { italic, bold }) => Ok(Self {
                italic: italic.unwrap_or(false),
                bold: bold.unwrap_or(false),
            }),
            Repr::Legacy(value) => value.parse().map_err(serde::de::Error::custom),
        }
    }
}

// ---------------------------------------------------------------------------
// Stroke
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextStrokeStyle {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_stroke_color")]
    pub color: [u8; 4],
    #[serde(default)]
    pub width_px: Option<f32>,
}

impl Default for TextStrokeStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [255, 255, 255, 255],
            width_px: None,
        }
    }
}

const fn default_true() -> bool {
    true
}

const fn default_stroke_color() -> [u8; 4] {
    [255, 255, 255, 255]
}

// ---------------------------------------------------------------------------
// Text style (scene-facing)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    pub font_families: Vec<String>,
    pub font_size: Option<f32>,
    pub color: [u8; 4],
    pub effect: Option<TextShaderEffect>,
    pub stroke: Option<TextStrokeStyle>,
    #[serde(default)]
    pub text_align: Option<TextAlign>,
}

#[cfg(test)]
mod tests {
    use super::TextShaderEffect;

    #[test]
    fn parse_combined_effects() {
        let effect: TextShaderEffect = "italic,bold".parse().expect("parse");
        assert!(effect.italic);
        assert!(effect.bold);
    }

    #[test]
    fn default_has_no_effects() {
        let effect = TextShaderEffect::default();
        assert!(!effect.italic);
        assert!(!effect.bold);
    }

    #[test]
    fn parse_none_disables_all_effects() {
        let effect: TextShaderEffect = "none".parse().expect("parse");
        assert_eq!(effect.to_string(), "none");
    }
}
