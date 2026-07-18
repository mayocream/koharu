//! Unicode script detection and shaping direction.

use harfrust::{Direction, Script, Tag};
use icu_properties::{
    CodePointMapData,
    props::{NamedEnumeratedProperty, Script as IcuScript},
};
use unicode_bidi::BidiInfo;

use crate::layout::WritingMode;
pub(crate) fn shaping_direction_for_text(
    text: &str,
    writing_mode: WritingMode,
) -> (Direction, Option<Script>) {
    if writing_mode.is_vertical() {
        return (
            Direction::TopToBottom,
            dominant_script(text).and_then(harfrust_script),
        );
    }

    let bidi = BidiInfo::new(text, None);
    let direction = if bidi
        .paragraphs
        .first()
        .is_some_and(|paragraph| paragraph.level.is_rtl())
    {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    };
    (direction, dominant_script(text).and_then(harfrust_script))
}

/// Returns the distinct ISO 15924 scripts Fontique should use for fallback.
pub(crate) fn fontique_scripts(text: &str) -> Vec<fontique::Script> {
    let script_map = CodePointMapData::<IcuScript>::new();
    let mut scripts = Vec::new();
    let mut has_emoji = false;
    for character in text.chars() {
        let script = script_map.get(character);
        if script == IcuScript::Common || script == IcuScript::Inherited {
            has_emoji |= character >= '\u{1F000}';
            continue;
        }
        let script = fallback_script(script);
        let Some(tag) = script_tag(script) else {
            continue;
        };
        let script = fontique::Script::from_bytes(tag);
        if !scripts.contains(&script) {
            scripts.push(script);
        }
    }
    if has_emoji {
        let emoji = fontique::Script::from_bytes(*b"Zsye");
        if !scripts.contains(&emoji) {
            scripts.push(emoji);
        }
    }
    if scripts.is_empty() {
        scripts.push(fontique::Script::from_bytes(*b"Latn"));
    }
    scripts
}

pub(crate) fn harfrust_script(script: IcuScript) -> Option<Script> {
    Script::from_iso15924_tag(Tag::new(&script_tag(fallback_script(script))?))
}

fn dominant_script(text: &str) -> Option<IcuScript> {
    let script_map = CodePointMapData::<IcuScript>::new();
    text.chars()
        .map(|character| script_map.get(character))
        .find(|script| *script != IcuScript::Common && *script != IcuScript::Inherited)
        .map(fallback_script)
}

fn fallback_script(script: IcuScript) -> IcuScript {
    match script {
        IcuScript::Hiragana | IcuScript::Katakana | IcuScript::Bopomofo => IcuScript::Han,
        other => other,
    }
}

fn script_tag(script: IcuScript) -> Option<[u8; 4]> {
    let bytes = script.short_name().as_bytes();
    (bytes.len() == 4).then(|| [bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub(crate) fn is_cjk_text(text: &str) -> bool {
    let script_map = CodePointMapData::<IcuScript>::new();
    text.chars().any(|character| {
        matches!(
            script_map.get(character),
            IcuScript::Han | IcuScript::Hiragana | IcuScript::Katakana | IcuScript::Bopomofo
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_scripts_preserve_cjk_locale_selection() {
        assert_eq!(
            fontique_scripts("日本語"),
            [fontique::Script::from_bytes(*b"Hani")]
        );
        assert_eq!(
            fontique_scripts("한국어"),
            [fontique::Script::from_bytes(*b"Hang")]
        );
        assert_eq!(
            fontique_scripts("مرحبا"),
            [fontique::Script::from_bytes(*b"Arab")]
        );
    }

    #[test]
    fn arabic_text_uses_rtl_shaping() {
        let (direction, script) = shaping_direction_for_text("مرحبا", WritingMode::Horizontal);
        assert_eq!(direction, Direction::RightToLeft);
        assert_eq!(script.unwrap().tag(), Tag::new(b"Arab"));
    }
}
