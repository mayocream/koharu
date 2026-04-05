//! Map Koharu [`Language`](koharu_llm::Language) targets to DeepL and Google Cloud Translation codes.

use koharu_llm::Language;

/// DeepL `target_lang` parameter (uppercase codes per DeepL API).
pub fn deepl_target_lang(language: Language) -> &'static str {
    match language {
        Language::ChineseSimplified => "ZH-HANS",
        Language::ChineseTraditional => "ZH-HANT",
        Language::English => "EN-US",
        Language::French => "FR",
        Language::Portuguese => "PT-PT",
        Language::BrazilianPortuguese => "PT-BR",
        Language::Spanish => "ES",
        Language::Japanese => "JA",
        Language::Turkish => "TR",
        Language::Russian => "RU",
        Language::Arabic => "AR",
        Language::Korean => "KO",
        Language::Thai => "TH",
        Language::Italian => "IT",
        Language::German => "DE",
        Language::Vietnamese => "VI",
        Language::Malay => "MS",
        Language::Indonesian => "ID",
        Language::Filipino => "EN-US",
        Language::Hindi => "HI",
        Language::Polish => "PL",
        Language::Czech => "CS",
        Language::Dutch => "NL",
        Language::Khmer => "EN-US",
        Language::Burmese => "EN-US",
        Language::Persian => "EN-US",
        Language::Gujarati => "GU",
        Language::Urdu => "UR",
        Language::Telugu => "TE",
        Language::Marathi => "MR",
        Language::Hebrew => "HE",
        Language::Bengali => "BN",
        Language::Bulgarian => "BG",
        Language::Tamil => "TA",
        Language::Ukrainian => "UK",
        Language::Tibetan => "ZH",
        Language::Kazakh => "KK",
        Language::Mongolian => "EN-US",
        Language::Uyghur => "ZH",
        Language::Cantonese => "ZH",
    }
}

/// Optional DeepL `source_lang` when we can infer from Koharu language tag.
pub fn deepl_source_lang(tag: Option<&str>) -> Option<&'static str> {
    let tag = tag?.trim();
    if tag.is_empty() {
        return None;
    }
    Language::parse(tag).and_then(|l| match l {
        Language::Japanese => Some("JA"),
        Language::ChineseSimplified => Some("ZH-HANS"),
        Language::ChineseTraditional => Some("ZH-HANT"),
        Language::English => Some("EN"),
        Language::Korean => Some("KO"),
        _ => None,
    })
}

/// Google Cloud Translation v2 `target` — BCP-47 language code.
pub fn google_target_language(language: Language) -> &'static str {
    language.tag()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepl_english_us() {
        assert_eq!(deepl_target_lang(Language::English), "EN-US");
    }

    #[test]
    fn deepl_japanese() {
        assert_eq!(deepl_target_lang(Language::Japanese), "JA");
    }

    #[test]
    fn google_matches_tag() {
        assert_eq!(google_target_language(Language::ChineseSimplified), "zh-CN");
    }

    #[test]
    fn deepl_chinese_script_codes() {
        assert_eq!(deepl_target_lang(Language::ChineseSimplified), "ZH-HANS");
        assert_eq!(deepl_target_lang(Language::ChineseTraditional), "ZH-HANT");
    }
}
