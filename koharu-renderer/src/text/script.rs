use harfrust::{Direction, Script, Tag};
use icu::properties::{CodePointMapData, props::Script as IcuScript};
use koharu_core::TextBlock;

use crate::layout::WritingMode;

pub fn writing_mode_for_block(text_block: &TextBlock) -> WritingMode {
    let text = match &text_block.translation {
        Some(t) => t,
        None => return WritingMode::Horizontal,
    };

    if !is_cjk_text(text) || text_block.width >= text_block.height {
        WritingMode::Horizontal
    } else {
        WritingMode::VerticalRl
    }
}

pub fn is_latin_only(text: &str) -> bool {
    let script_map = CodePointMapData::<IcuScript>::new();
    text.chars().all(|c| {
        matches!(
            script_map.get(c),
            IcuScript::Latin | IcuScript::Common | IcuScript::Inherited
        )
    })
}

pub fn normalize_translation_for_layout(text: &str) -> String {
    if is_latin_only(text) {
        text.to_uppercase()
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
            IcuScript::Arabic
            | IcuScript::Hebrew
            | IcuScript::Syriac
            | IcuScript::Thaana
            | IcuScript::Nko
            | IcuScript::Adlam => {
                if rtl_script.is_none() {
                    rtl_script = Some(script_map.get(c));
                }
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

    let flags = detect_scripts(text);
    if let Some(rtl) = flags.rtl_script {
        let tag = match rtl {
            IcuScript::Hebrew => b"Hebr",
            IcuScript::Syriac => b"Syrc",
            IcuScript::Thaana => b"Thaa",
            IcuScript::Nko => b"Nkoo",
            IcuScript::Adlam => b"Adlm",
            _ => b"Arab",
        };
        (
            Direction::RightToLeft,
            Script::from_iso15924_tag(Tag::new(tag)),
        )
    } else if flags.has_thai {
        (
            Direction::LeftToRight,
            Script::from_iso15924_tag(Tag::new(b"Thai")),
        )
    } else if flags.has_cjk {
        (
            Direction::LeftToRight,
            Script::from_iso15924_tag(Tag::new(b"Hani")),
        )
    } else {
        (Direction::LeftToRight, None)
    }
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
    use koharu_core::{TextBlock, TextDirection};

    use crate::layout::WritingMode;

    use super::{
        font_families_for_text, is_latin_only, normalize_translation_for_layout,
        shaping_direction_for_text, writing_mode_for_block,
    };

    #[test]
    fn latin_detection_is_reasonable() {
        assert!(is_latin_only("hello!?"));
        assert!(!is_latin_only("こんにちは"));
    }

    #[test]
    fn normalize_uppercases_latin_only() {
        assert_eq!(normalize_translation_for_layout("hello!"), "HELLO!");
        assert_eq!(normalize_translation_for_layout("中文"), "中文");
    }

    #[test]
    fn font_family_selection_returns_candidates() {
        assert!(!font_families_for_text("hello").is_empty());
        assert!(!font_families_for_text("你好").is_empty());
        assert!(!font_families_for_text("مرحبا").is_empty());
        assert!(!font_families_for_text("สวัสดี").is_empty());
    }

    #[test]
    fn writing_mode_uses_cjk_tall_box_heuristic() {
        let block = TextBlock {
            width: 40.0,
            height: 120.0,
            translation: Some("縦書き".to_string()),
            ..Default::default()
        };

        assert_eq!(writing_mode_for_block(&block), WritingMode::VerticalRl);
    }

    #[test]
    fn writing_mode_ignores_stale_rendered_direction() {
        let block = TextBlock {
            width: 40.0,
            height: 120.0,
            translation: Some("HELLO".to_string()),
            source_direction: Some(TextDirection::Horizontal),
            rendered_direction: Some(TextDirection::Vertical),
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
