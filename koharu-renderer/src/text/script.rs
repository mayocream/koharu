use harfrust::{Direction, Script, Tag};
use icu::properties::{CodePointMapData, props::Script as IcuScript};
use icu_casemap::CaseMapper;
use icu_locale::LanguageIdentifier;
use unicode_bidi::BidiInfo;

use crate::layout::WritingMode;
use crate::types::{RenderBlock, TextDirection};

pub fn writing_mode_for_block(block: &RenderBlock) -> WritingMode {
    if block.text.is_empty() {
        return WritingMode::Horizontal;
    }
    // Non-CJK text always lays out horizontally regardless of bubble shape —
    // an English translation in a tall manga bubble still reads left-to-right.
    if !is_cjk_text(&block.text) {
        return WritingMode::Horizontal;
    }
    // CJK content: prefer the OCR/detector's source direction when available,
    // so bubble aspect ratio doesn't override a trusted signal. The bbox
    // heuristic is kept only as a fallback for user-added blocks that have
    // no recorded source direction.
    match block.source_direction {
        Some(TextDirection::Vertical) => WritingMode::VerticalRl,
        Some(TextDirection::Horizontal) => WritingMode::Horizontal,
        None => {
            if block.height > block.width {
                WritingMode::VerticalRl
            } else {
                WritingMode::Horizontal
            }
        }
    }
}

pub fn normalize_translation_for_layout(text: &str, language: Option<&str>) -> String {
    if !is_cjk_text(text) {
        let mapper = CaseMapper::new();
        let lang_id: LanguageIdentifier = language
            .and_then(|l| l.parse().ok())
            .unwrap_or_else(|| "und".parse().unwrap());

        mapper.uppercase_to_string(text, &lang_id).to_string()
    } else {
        text.to_string()
    }
}

pub(crate) struct ScriptFlags {
    pub has_cjk: bool,
    pub rtl_script: Option<IcuScript>,
    pub has_thai: bool,
}

pub(crate) fn detect_scripts(text: &str) -> ScriptFlags {
    let script_map = CodePointMapData::<IcuScript>::new();
    let (mut has_cjk, mut rtl_script, mut has_thai) = (false, None, false);
    for c in text.chars() {
        match script_map.get(c) {
            IcuScript::Han
            | IcuScript::Hiragana
            | IcuScript::Katakana
            | IcuScript::Hangul
            | IcuScript::Bopomofo => has_cjk = true,
            rtl @ (IcuScript::Arabic
            | IcuScript::Hebrew
            | IcuScript::Syriac
            | IcuScript::Thaana
            | IcuScript::Nko
            | IcuScript::Adlam)
                if rtl_script.is_none() =>
            {
                rtl_script = Some(rtl);
            }
            IcuScript::Thai | IcuScript::Lao | IcuScript::Khmer | IcuScript::Myanmar => {
                has_thai = true
            }
            _ => {}
        }
        if has_cjk && rtl_script.is_some() && has_thai {
            break;
        }
    }
    ScriptFlags {
        has_cjk,
        rtl_script,
        has_thai,
    }
}

pub fn shaping_direction_for_text(
    text: &str,
    writing_mode: WritingMode,
) -> (Direction, Option<Script>) {
    if writing_mode.is_vertical() {
        return (Direction::TopToBottom, None);
    }

    if text.is_empty() {
        return (Direction::LeftToRight, None);
    }

    // Use unicode-bidi to detect the paragraph's base direction.
    let bidi_info = BidiInfo::new(text, None);
    let direction = if !bidi_info.paragraphs.is_empty() && bidi_info.paragraphs[0].level.is_rtl() {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    };

    let flags = detect_scripts(text);
    let script = if let Some(rtl) = flags.rtl_script {
        let tag = match rtl {
            IcuScript::Hebrew => b"Hebr",
            IcuScript::Syriac => b"Syrc",
            IcuScript::Thaana => b"Thaa",
            IcuScript::Nko => b"Nkoo",
            IcuScript::Adlam => b"Adlm",
            _ => b"Arab",
        };
        Script::from_iso15924_tag(Tag::new(tag))
    } else if flags.has_thai {
        Script::from_iso15924_tag(Tag::new(b"Thai"))
    } else if flags.has_cjk {
        Script::from_iso15924_tag(Tag::new(b"Hani"))
    } else {
        None
    };

    (direction, script)
}

pub fn font_families_for_text(text: &str) -> Vec<String> {
    let ScriptFlags {
        has_cjk,
        rtl_script,
        has_thai,
    } = detect_scripts(text);

    let names: &[&str] = if has_cjk {
        #[cfg(target_os = "windows")]
        {
            &["Microsoft YaHei"]
        }
        #[cfg(target_os = "macos")]
        {
            &["PingFang SC"]
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            &["Noto Sans CJK SC"]
        }
    } else if rtl_script.is_some() {
        #[cfg(target_os = "windows")]
        {
            &["Segoe UI"]
        }
        #[cfg(target_os = "macos")]
        {
            &["SF Pro"]
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            &["Noto Sans"]
        }
    } else if has_thai {
        #[cfg(target_os = "windows")]
        {
            &["Leelawadee UI", "Leelawadee", "Tahoma"]
        }
        #[cfg(target_os = "macos")]
        {
            &["Thonburi", "Ayuthaya"]
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            &["Noto Sans Thai", "Noto Sans"]
        }
    } else {
        // Google Fonts candidates (downloaded on demand) + platform fallbacks
        #[cfg(target_os = "windows")]
        {
            &[
                "Comic Neue",
                "Bangers",
                "Comic Sans MS",
                "Trebuchet MS",
                "Segoe UI",
                "Arial",
            ]
        }
        #[cfg(target_os = "macos")]
        {
            &[
                "Comic Neue",
                "Bangers",
                "Chalkboard SE",
                "Noteworthy",
                "SF Pro",
                "Helvetica",
            ]
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            &[
                "Comic Neue",
                "Bangers",
                "Noto Sans",
                "DejaVu Sans",
                "Liberation Sans",
            ]
        }
    };

    names.iter().map(|s| s.to_string()).collect()
}

fn is_cjk_text(text: &str) -> bool {
    let script_map = CodePointMapData::<IcuScript>::new();
    text.chars().any(|c| {
        matches!(
            script_map.get(c),
            IcuScript::Han | IcuScript::Hiragana | IcuScript::Katakana | IcuScript::Bopomofo
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::layout::WritingMode;
    use crate::types::RenderBlock;

    use super::{
        font_families_for_text, normalize_translation_for_layout, shaping_direction_for_text,
        writing_mode_for_block,
    };

    #[test]
    fn normalize_uppercases_latin_only() {
        assert_eq!(normalize_translation_for_layout("hello!", None), "HELLO!");
        assert_eq!(normalize_translation_for_layout("中文", None), "中文");
    }

    #[test]
    fn normalize_handles_turkish_i() {
        assert_eq!(
            normalize_translation_for_layout("kimse", Some("tr")),
            "KİMSE"
        );
        // Test with subtag
        assert_eq!(
            normalize_translation_for_layout("ışık", Some("tr-TR")),
            "IŞIK"
        );
    }

    #[test]
    fn normalize_handles_other_scripts() {
        // Cyrillic
        assert_eq!(normalize_translation_for_layout("привет", None), "ПРИВЕТ");
        // Greek
        assert_eq!(
            normalize_translation_for_layout("γειά σου", None),
            "ΓΕΙΆ ΣΟΥ"
        );
    }

    #[test]
    fn normalize_skips_cjk_scripts() {
        assert_eq!(normalize_translation_for_layout("你好", None), "你好");
        assert_eq!(
            normalize_translation_for_layout("안녕하세요", None),
            "안녕하세요"
        );
    }

    #[test]
    fn font_family_selection_returns_candidates() {
        assert!(!font_families_for_text("hello").is_empty());
        assert!(!font_families_for_text("你好").is_empty());
        assert!(!font_families_for_text("مرحبا").is_empty());
        assert!(!font_families_for_text("สวัสดี").is_empty());
    }

    #[test]
    fn writing_mode_falls_back_to_cjk_tall_box_when_source_direction_missing() {
        let block = RenderBlock {
            width: 40.0,
            height: 120.0,
            text: "縦書き".to_string(),
            ..Default::default()
        };

        assert_eq!(writing_mode_for_block(&block), WritingMode::VerticalRl);
    }

    #[test]
    fn writing_mode_uses_latin_text_even_in_tall_box() {
        let block = RenderBlock {
            width: 40.0,
            height: 120.0,
            text: "HELLO".to_string(),
            ..Default::default()
        };

        assert_eq!(writing_mode_for_block(&block), WritingMode::Horizontal);
    }

    #[test]
    fn writing_mode_honors_vertical_source_direction_in_wide_cjk_bubble() {
        // A wide bubble (width > height) with vertical source direction
        // should stay vertical — old bbox heuristic would have flipped it.
        let block = RenderBlock {
            width: 200.0,
            height: 60.0,
            text: "縦書き".to_string(),
            source_direction: Some(crate::types::TextDirection::Vertical),
            ..Default::default()
        };

        assert_eq!(writing_mode_for_block(&block), WritingMode::VerticalRl);
    }

    #[test]
    fn writing_mode_honors_horizontal_source_direction_in_tall_cjk_bubble() {
        // A tall bubble with horizontal source direction should stay
        // horizontal — old bbox heuristic would have flipped it.
        let block = RenderBlock {
            width: 40.0,
            height: 120.0,
            text: "横書き".to_string(),
            source_direction: Some(crate::types::TextDirection::Horizontal),
            ..Default::default()
        };

        assert_eq!(writing_mode_for_block(&block), WritingMode::Horizontal);
    }

    #[test]
    fn arabic_text_uses_rtl_shaping() {
        let (dir, script) = shaping_direction_for_text("مرحبا", WritingMode::Horizontal);
        assert_eq!(dir, harfrust::Direction::RightToLeft);
        assert_eq!(script.unwrap().tag(), harfrust::Tag::new(b"Arab"));
    }

    #[test]
    fn hebrew_text_uses_rtl_shaping_with_correct_tag() {
        let (dir, script) = shaping_direction_for_text("שלום", WritingMode::Horizontal);
        assert_eq!(dir, harfrust::Direction::RightToLeft);
        assert_eq!(script.unwrap().tag(), harfrust::Tag::new(b"Hebr"));
    }

    #[test]
    fn syriac_text_uses_rtl_shaping_with_correct_tag() {
        let (dir, script) = shaping_direction_for_text("ܐܒܓܕ", WritingMode::Horizontal);
        assert_eq!(dir, harfrust::Direction::RightToLeft);
        assert_eq!(script.unwrap().tag(), harfrust::Tag::new(b"Syrc"));
    }

    #[test]
    fn latin_text_stays_ltr_shaping() {
        let (dir, script) = shaping_direction_for_text("HELLO", WritingMode::Horizontal);
        assert_eq!(dir, harfrust::Direction::LeftToRight);
        assert!(script.is_none());
    }
}
