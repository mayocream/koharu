use icu::properties::{CodePointMapData, props::Script};
use koharu_types::TextBlock;

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

pub fn font_families_for_text(text: &str) -> Vec<String> {
    let script_map = CodePointMapData::<Script>::new();
    let has_cjk = text.chars().any(|c| {
        matches!(
            script_map.get(c),
            Script::Han | Script::Hiragana | Script::Katakana | Script::Hangul | Script::Bopomofo
        )
    });
    let has_arabic = text
        .chars()
        .any(|c| matches!(script_map.get(c), Script::Arabic | Script::Hebrew));

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
    } else {
        #[cfg(target_os = "windows")]
        {
            &[
                "CC Wild Words",
                "Wild Words",
                "Anime Ace 2.0 BB",
                "Comic Sans MS",
                "Trebuchet MS",
                "Segoe UI",
                "Arial",
            ]
        }
        #[cfg(target_os = "macos")]
        {
            &[
                "CC Wild Words",
                "Wild Words",
                "Anime Ace 2.0 BB",
                "Chalkboard SE",
                "Noteworthy",
                "SF Pro",
                "Helvetica",
            ]
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            &[
                "CC Wild Words",
                "Wild Words",
                "Anime Ace 2.0 BB",
                "Comic Neue",
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
    use super::{font_families_for_text, is_latin_only, normalize_translation_for_layout};

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
    }
}
