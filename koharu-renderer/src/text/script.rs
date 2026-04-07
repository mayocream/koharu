use icu::properties::{CodePointMapData, props::Script};
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
    let script_map = CodePointMapData::<Script>::new();
    text.chars().all(|c| {
        matches!(
            script_map.get(c),
            Script::Latin | Script::Common | Script::Inherited
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
    pub has_arabic: bool,
    pub has_thai: bool,
}

pub(crate) fn detect_scripts(text: &str) -> ScriptFlags {
    let script_map = CodePointMapData::<Script>::new();
    let (mut has_cjk, mut has_arabic, mut has_thai) = (false, false, false);
    for c in text.chars() {
        match script_map.get(c) {
            Script::Han
            | Script::Hiragana
            | Script::Katakana
            | Script::Hangul
            | Script::Bopomofo => has_cjk = true,
            Script::Arabic
            | Script::Hebrew
            | Script::Syriac
            | Script::Thaana
            | Script::Nko
            | Script::Adlam => has_arabic = true,
            Script::Thai | Script::Lao | Script::Khmer | Script::Myanmar => has_thai = true,
            _ => {}
        }
        if has_cjk && has_arabic && has_thai {
            break;
        }
    }
    ScriptFlags {
        has_cjk,
        has_arabic,
        has_thai,
    }
}

pub fn font_families_for_text(text: &str) -> Vec<String> {
    let ScriptFlags {
        has_cjk,
        has_arabic,
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
    } else if has_arabic {
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
    let script_map = CodePointMapData::<Script>::new();
    text.chars().any(|c| {
        matches!(
            script_map.get(c),
            Script::Han | Script::Hiragana | Script::Katakana | Script::Bopomofo
        )
    })
}

#[cfg(test)]
mod tests {
    use koharu_core::{TextBlock, TextDirection};

    use crate::layout::WritingMode;

    use super::{
        font_families_for_text, is_latin_only, normalize_translation_for_layout,
        writing_mode_for_block,
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
}
