use std::str::FromStr;

use strum::{Display, EnumIter, EnumProperty, EnumString, IntoEnumIterator};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, EnumIter, EnumProperty)]
pub enum Language {
    #[strum(
        to_string = "Simplified Chinese",
        serialize = "zh-CN",
        serialize = "zh",
        serialize = "zh-Hans",
        props(tag = "zh-CN")
    )]
    ChineseSimplified,
    #[strum(
        to_string = "English",
        serialize = "en-US",
        serialize = "en",
        props(tag = "en-US")
    )]
    English,
    #[strum(
        to_string = "French",
        serialize = "fr-FR",
        serialize = "fr",
        props(tag = "fr-FR")
    )]
    French,
    #[strum(
        to_string = "Portuguese",
        serialize = "pt-PT",
        serialize = "pt",
        props(tag = "pt-PT")
    )]
    Portuguese,
    #[strum(
        to_string = "Brazilian Portuguese",
        serialize = "pt-BR",
        props(tag = "pt-BR")
    )]
    BrazilianPortuguese,
    #[strum(
        to_string = "Spanish",
        serialize = "es-ES",
        serialize = "es",
        props(tag = "es-ES")
    )]
    Spanish,
    #[strum(
        to_string = "Japanese",
        serialize = "ja-JP",
        serialize = "ja",
        props(tag = "ja-JP")
    )]
    Japanese,
    #[strum(
        to_string = "Turkish",
        serialize = "tr-TR",
        serialize = "tr",
        props(tag = "tr-TR")
    )]
    Turkish,
    #[strum(
        to_string = "Russian",
        serialize = "ru-RU",
        serialize = "ru",
        props(tag = "ru-RU")
    )]
    Russian,
    #[strum(
        to_string = "Arabic",
        serialize = "ar-SA",
        serialize = "ar",
        props(tag = "ar-SA")
    )]
    Arabic,
    #[strum(
        to_string = "Korean",
        serialize = "ko-KR",
        serialize = "ko",
        props(tag = "ko-KR")
    )]
    Korean,
    #[strum(
        to_string = "Thai",
        serialize = "th-TH",
        serialize = "th",
        props(tag = "th-TH")
    )]
    Thai,
    #[strum(
        to_string = "Italian",
        serialize = "it-IT",
        serialize = "it",
        props(tag = "it-IT")
    )]
    Italian,
    #[strum(
        to_string = "German",
        serialize = "de-DE",
        serialize = "de",
        props(tag = "de-DE")
    )]
    German,
    #[strum(
        to_string = "Vietnamese",
        serialize = "vi-VN",
        serialize = "vi",
        props(tag = "vi-VN")
    )]
    Vietnamese,
    #[strum(
        to_string = "Malay",
        serialize = "ms-MY",
        serialize = "ms",
        props(tag = "ms-MY")
    )]
    Malay,
    #[strum(
        to_string = "Indonesian",
        serialize = "id-ID",
        serialize = "id",
        props(tag = "id-ID")
    )]
    Indonesian,
    #[strum(
        to_string = "Filipino",
        serialize = "fil-PH",
        serialize = "fil",
        serialize = "tl",
        props(tag = "fil-PH")
    )]
    Filipino,
    #[strum(
        to_string = "Hindi",
        serialize = "hi-IN",
        serialize = "hi",
        props(tag = "hi-IN")
    )]
    Hindi,
    #[strum(
        to_string = "Traditional Chinese",
        serialize = "zh-TW",
        serialize = "zh-Hant",
        props(tag = "zh-TW")
    )]
    ChineseTraditional,
    #[strum(
        to_string = "Polish",
        serialize = "pl-PL",
        serialize = "pl",
        props(tag = "pl-PL")
    )]
    Polish,
    #[strum(
        to_string = "Czech",
        serialize = "cs-CZ",
        serialize = "cs",
        props(tag = "cs-CZ")
    )]
    Czech,
    #[strum(
        to_string = "Dutch",
        serialize = "nl-NL",
        serialize = "nl",
        props(tag = "nl-NL")
    )]
    Dutch,
    #[strum(
        to_string = "Khmer",
        serialize = "km-KH",
        serialize = "km",
        props(tag = "km-KH")
    )]
    Khmer,
    #[strum(
        to_string = "Burmese",
        serialize = "my-MM",
        serialize = "my",
        props(tag = "my-MM")
    )]
    Burmese,
    #[strum(
        to_string = "Persian",
        serialize = "fa-IR",
        serialize = "fa",
        props(tag = "fa-IR")
    )]
    Persian,
    #[strum(
        to_string = "Gujarati",
        serialize = "gu-IN",
        serialize = "gu",
        props(tag = "gu-IN")
    )]
    Gujarati,
    #[strum(
        to_string = "Urdu",
        serialize = "ur-PK",
        serialize = "ur",
        props(tag = "ur-PK")
    )]
    Urdu,
    #[strum(
        to_string = "Telugu",
        serialize = "te-IN",
        serialize = "te",
        props(tag = "te-IN")
    )]
    Telugu,
    #[strum(
        to_string = "Marathi",
        serialize = "mr-IN",
        serialize = "mr",
        props(tag = "mr-IN")
    )]
    Marathi,
    #[strum(
        to_string = "Hebrew",
        serialize = "he-IL",
        serialize = "he",
        props(tag = "he-IL")
    )]
    Hebrew,
    #[strum(
        to_string = "Bengali",
        serialize = "bn-BD",
        serialize = "bn",
        props(tag = "bn-BD")
    )]
    Bengali,
    #[strum(
        to_string = "Bulgarian",
        serialize = "bg-BG",
        serialize = "bg",
        props(tag = "bg-BG")
    )]
    Bulgarian,
    #[strum(
        to_string = "Tamil",
        serialize = "ta-IN",
        serialize = "ta",
        props(tag = "ta-IN")
    )]
    Tamil,
    #[strum(
        to_string = "Ukrainian",
        serialize = "uk-UA",
        serialize = "uk",
        props(tag = "uk-UA")
    )]
    Ukrainian,
    #[strum(
        to_string = "Tibetan",
        serialize = "bo-CN",
        serialize = "bo",
        props(tag = "bo-CN")
    )]
    Tibetan,
    #[strum(
        to_string = "Kazakh",
        serialize = "kk-KZ",
        serialize = "kk",
        props(tag = "kk-KZ")
    )]
    Kazakh,
    #[strum(
        to_string = "Mongolian",
        serialize = "mn-MN",
        serialize = "mn",
        props(tag = "mn-MN")
    )]
    Mongolian,
    #[strum(
        to_string = "Uyghur",
        serialize = "ug-CN",
        serialize = "ug",
        props(tag = "ug-CN")
    )]
    Uyghur,
    #[strum(
        to_string = "Cantonese",
        serialize = "yue-HK",
        serialize = "yue",
        props(tag = "yue-HK")
    )]
    Cantonese,
}

impl Language {
    pub fn tag(self) -> &'static str {
        self.get_str("tag").expect("language tag property")
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        Self::from_str(value).ok()
    }
}

pub fn supported_locales() -> Vec<String> {
    Language::iter()
        .map(|language| language.tag().to_string())
        .collect()
}

pub fn language_from_tag(value: &str) -> String {
    Language::parse(value)
        .unwrap_or(Language::English)
        .to_string()
}

pub fn tags(languages: &[Language]) -> Vec<String> {
    languages
        .iter()
        .map(|language| language.tag().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Language, language_from_tag, supported_locales, tags};

    #[test]
    fn parses_tags_aliases_and_english_names() {
        assert_eq!(Language::parse("zh-CN"), Some(Language::ChineseSimplified));
        assert_eq!(Language::parse("zh"), Some(Language::ChineseSimplified));
        assert_eq!(
            Language::parse("Simplified Chinese"),
            Some(Language::ChineseSimplified)
        );
        assert_eq!(Language::parse("fil-PH"), Some(Language::Filipino));
        assert_eq!(Language::parse("tl"), Some(Language::Filipino));
        assert_eq!(Language::parse("bg-BG"), Some(Language::Bulgarian));
        assert_eq!(Language::parse("bg"), Some(Language::Bulgarian));
    }

    #[test]
    fn supported_locales_returns_tags() {
        let locales = supported_locales();
        assert!(locales.contains(&"en-US".to_string()));
        assert!(locales.contains(&"bg-BG".to_string()));
        assert!(locales.contains(&"zh-CN".to_string()));
        assert!(locales.contains(&"yue-HK".to_string()));
    }

    #[test]
    fn helper_functions_use_canonical_tags_and_english_names() {
        assert_eq!(language_from_tag("zh-TW"), "Traditional Chinese");
        assert_eq!(
            tags(&[Language::English, Language::Japanese]),
            vec!["en-US".to_string(), "ja-JP".to_string()]
        );
    }
}
