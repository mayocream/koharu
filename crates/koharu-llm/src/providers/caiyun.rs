//! Caiyun LingoCloud translation API (`/v1/translator`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Context;
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};

use crate::Language;

use super::AnyProvider;

const CAIYUN_TRANSLATOR_URL: &str = "https://api.interpreter.caiyunai.com/v1/translator";
const REQUEST_ID: &str = "koharu-caiyun";

macro_rules! caiyun_target_languages {
    ($( $language:ident => $code:literal ),* $(,)?) => {
        pub const SUPPORTED_TARGET_LANGUAGES: &[Language] = &[
            $(Language::$language),*
        ];

        fn caiyun_target_lang(language: Language) -> Option<&'static str> {
            match language {
                $(Language::$language => Some($code),)*
                _ => None,
            }
        }
    };
}

caiyun_target_languages!(
    ChineseSimplified => "zh",
    English => "en",
    French => "fr",
    Portuguese => "pt",
    Spanish => "es",
    Japanese => "ja",
    Turkish => "tr",
    Russian => "ru",
    Arabic => "ar",
    Korean => "ko",
    Thai => "th",
    Italian => "it",
    German => "de",
    Vietnamese => "vi",
    Indonesian => "id",
    ChineseTraditional => "zh-Hant",
    Polish => "pl",
);

#[derive(Debug, Serialize)]
struct CaiyunRequest<'a> {
    source: Vec<&'a str>,
    trans_type: String,
    request_id: &'static str,
    detect: bool,
    media: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CaiyunTarget {
    One(String),
    Many(Vec<String>),
}

impl CaiyunTarget {
    fn into_first(self) -> Option<String> {
        match self {
            Self::One(text) => Some(text),
            Self::Many(targets) => targets.into_iter().next(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CaiyunResponse {
    #[serde(default)]
    rc: i64,
    #[serde(default)]
    target: Option<CaiyunTarget>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

pub struct CaiyunMtProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: String,
}

impl AnyProvider for CaiyunMtProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        _model: &'a str,
        _custom_system_prompt: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let target = caiyun_target_lang(target_language).ok_or_else(|| {
                anyhow::anyhow!(
                    "Caiyun does not support target language {}",
                    target_language.tag()
                )
            })?;
            let body = CaiyunRequest {
                source: vec![source],
                trans_type: format!("auto2{target}"),
                request_id: REQUEST_ID,
                detect: true,
                media: "text",
            };
            let json = serde_json::to_vec(&body).context("serialize Caiyun request body")?;

            let response = self
                .http_client
                .post(CAIYUN_TRANSLATOR_URL)
                .header("Content-Type", "application/json")
                .header("X-Authorization", format!("token {}", self.api_key))
                .body(json)
                .send()
                .await
                .context("Caiyun translate request")?;

            let status = response.status();
            let response_text = response.text().await.context("Caiyun response body")?;
            if !status.is_success() {
                if status == reqwest::StatusCode::CONFLICT && response_text.trim().is_empty() {
                    anyhow::bail!(
                        "Caiyun API failed ({status}): unsupported target language or translation direction"
                    );
                }
                anyhow::bail!("Caiyun API failed ({status}): {response_text}");
            }

            let parsed: CaiyunResponse = serde_json::from_str(&response_text)
                .with_context(|| format!("Caiyun JSON parse: {response_text}"))?;
            if parsed.rc != 0 {
                let detail = parsed
                    .msg
                    .or(parsed.message)
                    .or(parsed.error)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| response_text.clone());
                anyhow::bail!("Caiyun API returned rc={}: {detail}", parsed.rc);
            }

            parsed
                .target
                .and_then(CaiyunTarget::into_first)
                .ok_or_else(|| anyhow::anyhow!("Caiyun returned no translations"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::caiyun_target_lang;
    use crate::Language;

    #[test]
    fn maps_supported_target_languages() {
        assert_eq!(caiyun_target_lang(Language::ChineseSimplified), Some("zh"));
        assert_eq!(caiyun_target_lang(Language::English), Some("en"));
        assert_eq!(caiyun_target_lang(Language::Japanese), Some("ja"));
        assert_eq!(
            caiyun_target_lang(Language::ChineseTraditional),
            Some("zh-Hant")
        );
        assert_eq!(caiyun_target_lang(Language::Polish), Some("pl"));
    }

    #[test]
    fn rejects_unsupported_target_languages() {
        assert_eq!(caiyun_target_lang(Language::BrazilianPortuguese), None);
        assert_eq!(caiyun_target_lang(Language::Hebrew), None);
        assert_eq!(caiyun_target_lang(Language::Cantonese), None);
    }
}
