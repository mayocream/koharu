use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TextShaderEffect {
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub border: bool,
}

impl TextShaderEffect {
    pub const ITALIC_FLAG: u32 = 1 << 0;
    pub const BOLD_FLAG: u32 = 1 << 1;
    pub const BORDER_FLAG: u32 = 1 << 2;

    pub fn flags(self) -> u32 {
        let mut flags = 0u32;
        if self.italic {
            flags |= Self::ITALIC_FLAG;
        }
        if self.bold {
            flags |= Self::BOLD_FLAG;
        }
        if self.border {
            flags |= Self::BORDER_FLAG;
        }
        flags
    }

    pub fn is_empty(self) -> bool {
        self.flags() == 0
    }
}

impl fmt::Display for TextShaderEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        if self.italic {
            parts.push("italic");
        }
        if self.bold {
            parts.push("bold");
        }
        if self.border {
            parts.push("border");
        }

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
            return Ok(Self::default());
        }

        let legacy = ["antique", "metal", "manga", "motionblur", "motion_blur"];
        if legacy.contains(&normalized.as_str()) {
            return Ok(Self::default());
        }

        let mut effect = Self::default();
        for token in normalized
            .split(|c: char| c == ',' || c == '|' || c == '+' || c.is_whitespace())
            .filter(|token| !token.is_empty())
        {
            match token {
                "italic" => effect.italic = true,
                "bold" => effect.bold = true,
                "border" | "outline" | "stroke" => effect.border = true,
                // Legacy aliases map to no effect for compatibility with old projects/configs.
                "antique" | "metal" | "manga" | "motionblur" | "motion_blur" | "normal"
                | "none" => {}
                _ => anyhow::bail!("Unknown shader effect: {token}. Valid: italic, bold, border"),
            }
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
        #[serde(untagged)]
        enum Repr {
            Flags {
                #[serde(default)]
                italic: bool,
                #[serde(default)]
                bold: bool,
                #[serde(default)]
                border: bool,
            },
            Legacy(String),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Flags {
                italic,
                bold,
                border,
            } => Ok(Self {
                italic,
                bold,
                border,
            }),
            Repr::Legacy(value) => value.parse().map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TextShaderEffect;

    #[test]
    fn parse_combined_effects() {
        let effect: TextShaderEffect = "italic,bold,border".parse().expect("parse");
        assert!(effect.italic);
        assert!(effect.bold);
        assert!(effect.border);
    }

    #[test]
    fn parse_legacy_effects_to_none() {
        let effect: TextShaderEffect = "manga".parse().expect("parse");
        assert!(effect.is_empty());
    }

    #[test]
    fn display_none() {
        let effect = TextShaderEffect::default();
        assert_eq!(effect.to_string(), "none");
    }
}
