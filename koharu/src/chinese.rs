//! Chinese text conversion module.
//!
//! Provides conversion between Simplified and Traditional Chinese variants
//! using OpenCC (Open Chinese Convert).

use std::sync::OnceLock;

use anyhow::Result;
use ferrous_opencc::{OpenCC, config::BuiltinConfig};
use serde::{Deserialize, Serialize};

/// Chinese conversion profile
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChineseVariant {
    /// No conversion (pass-through)
    #[default]
    None,
    /// Simplified Chinese
    Simplified,
    /// Traditional Chinese (generic)
    Traditional,
    /// Traditional Chinese (Taiwan standard)
    TraditionalTw,
    /// Traditional Chinese (Taiwan with phrase conversion)
    TraditionalTwp,
    /// Traditional Chinese (Hong Kong standard)
    TraditionalHk,
}

impl ChineseVariant {
    /// Returns the display name for this variant
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Simplified => "Simplified Chinese",
            Self::Traditional => "Traditional Chinese",
            Self::TraditionalTw => "Traditional Chinese (Taiwan)",
            Self::TraditionalTwp => "Traditional Chinese (Taiwan+)",
            Self::TraditionalHk => "Traditional Chinese (Hong Kong)",
        }
    }
}

/// Cached OpenCC converters for each conversion path
struct Converters {
    s2t: OnceLock<Result<OpenCC, String>>,
    s2tw: OnceLock<Result<OpenCC, String>>,
    s2twp: OnceLock<Result<OpenCC, String>>,
    s2hk: OnceLock<Result<OpenCC, String>>,
    t2s: OnceLock<Result<OpenCC, String>>,
    tw2s: OnceLock<Result<OpenCC, String>>,
    hk2s: OnceLock<Result<OpenCC, String>>,
}

impl Converters {
    const fn new() -> Self {
        Self {
            s2t: OnceLock::new(),
            s2tw: OnceLock::new(),
            s2twp: OnceLock::new(),
            s2hk: OnceLock::new(),
            t2s: OnceLock::new(),
            tw2s: OnceLock::new(),
            hk2s: OnceLock::new(),
        }
    }
}

static CONVERTERS: Converters = Converters::new();

/// Get or initialize a converter for the given config
fn get_converter(config: BuiltinConfig) -> Result<&'static OpenCC> {
    let lock = match config {
        BuiltinConfig::S2t => &CONVERTERS.s2t,
        BuiltinConfig::S2tw => &CONVERTERS.s2tw,
        BuiltinConfig::S2twp => &CONVERTERS.s2twp,
        BuiltinConfig::S2hk => &CONVERTERS.s2hk,
        BuiltinConfig::T2s => &CONVERTERS.t2s,
        BuiltinConfig::Tw2s => &CONVERTERS.tw2s,
        BuiltinConfig::Hk2s => &CONVERTERS.hk2s,
        _ => anyhow::bail!("Unsupported conversion config: {:?}", config),
    };

    let result = lock.get_or_init(|| {
        OpenCC::from_config(config)
            .map_err(|e| format!("Failed to initialize OpenCC for {:?}: {}", config, e))
    });

    match result {
        Ok(converter) => Ok(converter),
        Err(e) => anyhow::bail!("{}", e),
    }
}

/// Convert text from Simplified Chinese to the target variant
pub fn convert_from_simplified(text: &str, target: ChineseVariant) -> Result<String> {
    let config = match target {
        ChineseVariant::None | ChineseVariant::Simplified => return Ok(text.to_string()),
        ChineseVariant::Traditional => BuiltinConfig::S2t,
        ChineseVariant::TraditionalTw => BuiltinConfig::S2tw,
        ChineseVariant::TraditionalTwp => BuiltinConfig::S2twp,
        ChineseVariant::TraditionalHk => BuiltinConfig::S2hk,
    };

    let converter = get_converter(config)?;
    Ok(converter.convert(text))
}

/// Convert text from Traditional Chinese to Simplified
pub fn convert_to_simplified(text: &str, source: ChineseVariant) -> Result<String> {
    let config = match source {
        ChineseVariant::None | ChineseVariant::Simplified => return Ok(text.to_string()),
        ChineseVariant::Traditional => BuiltinConfig::T2s,
        ChineseVariant::TraditionalTw | ChineseVariant::TraditionalTwp => BuiltinConfig::Tw2s,
        ChineseVariant::TraditionalHk => BuiltinConfig::Hk2s,
    };

    let converter = get_converter(config)?;
    Ok(converter.convert(text))
}

/// Convert text between any two Chinese variants
pub fn convert(text: &str, from: ChineseVariant, to: ChineseVariant) -> Result<String> {
    if from == to {
        return Ok(text.to_string());
    }

    // If converting from simplified
    if from == ChineseVariant::Simplified || from == ChineseVariant::None {
        return convert_from_simplified(text, to);
    }

    // If converting to simplified
    if to == ChineseVariant::Simplified {
        return convert_to_simplified(text, from);
    }

    // Converting between traditional variants: go through simplified first
    let simplified = convert_to_simplified(text, from)?;
    convert_from_simplified(&simplified, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s2hk_conversion() {
        let simplified = "简体中文转换测试";
        let result = convert_from_simplified(simplified, ChineseVariant::TraditionalHk).unwrap();
        // Should contain traditional characters
        assert!(result.contains('繁') || result.contains('簡') || result != simplified);
        assert_ne!(result, simplified);
    }

    #[test]
    fn test_s2tw_conversion() {
        let simplified = "软件开发";
        let result = convert_from_simplified(simplified, ChineseVariant::TraditionalTw).unwrap();
        assert_ne!(result, simplified);
    }

    #[test]
    fn test_no_conversion() {
        let text = "Hello World 你好世界";
        let result = convert_from_simplified(text, ChineseVariant::None).unwrap();
        assert_eq!(result, text);
    }

    #[test]
    fn test_variant_display_names() {
        assert_eq!(ChineseVariant::None.display_name(), "None");
        assert_eq!(ChineseVariant::TraditionalHk.display_name(), "Traditional Chinese (Hong Kong)");
    }

    #[test]
    fn test_round_trip() {
        let original = "计算机软件";
        let traditional = convert_from_simplified(original, ChineseVariant::Traditional).unwrap();
        let back = convert_to_simplified(&traditional, ChineseVariant::Traditional).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn test_s2hk_specific_characters() {
        // Test specific character conversions for Hong Kong Traditional
        let cases = [
            ("软件", "軟件"),     // software
            ("网络", "網絡"),     // network (HK uses 絡 not 路)
            ("计算机", "計算機"), // computer
        ];

        for (simplified, expected_traditional) in cases {
            let result = convert_from_simplified(simplified, ChineseVariant::TraditionalHk).unwrap();
            assert_eq!(result, expected_traditional, "Failed for: {}", simplified);
        }
    }

    #[test]
    fn test_manga_chapter_title() {
        // Test realistic manga chapter title conversion
        let simplified = "第1178话 大结局";
        let result = convert_from_simplified(simplified, ChineseVariant::TraditionalHk).unwrap();
        // Numbers should be preserved, Chinese characters should be converted
        assert!(result.contains("1178"));
        assert!(result.contains("話")); // 话 -> 話
        assert!(result.contains("結")); // 结 -> 結
    }
}
