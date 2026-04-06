//! DeepL REST API (`/v2/translate`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Context;
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;

use crate::Language;

use super::AnyProvider;

const DEFAULT_BASE_URL: &str = "https://api.deepl.com";

#[derive(Debug, Deserialize)]
struct DeeplResponse {
    translations: Vec<DeeplTranslation>,
}

#[derive(Debug, Deserialize)]
struct DeeplTranslation {
    text: String,
}

fn normalize_base_url(base: Option<&str>) -> String {
    base.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

fn deepl_target_lang(language: Language) -> &'static str {
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

pub struct DeeplMtProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: String,
    pub base_url: Option<String>,
}

impl AnyProvider for DeeplMtProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        _model: &'a str,
        _custom_system_prompt: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let root = normalize_base_url(self.base_url.as_deref());
            let url = format!("{root}/v2/translate");

            let encoded = {
                let mut ser = url::form_urlencoded::Serializer::new(String::new());
                ser.append_pair("text", source);
                ser.append_pair("target_lang", deepl_target_lang(target_language));
                ser.finish()
            };

            let response = self
                .http_client
                .post(&url)
                .header("Authorization", format!("DeepL-Auth-Key {}", self.api_key))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(encoded)
                .send()
                .await
                .context("DeepL translate request")?;

            let status = response.status();
            let response_text = response.text().await.context("DeepL response body")?;
            if !status.is_success() {
                anyhow::bail!("DeepL API failed ({status}): {response_text}");
            }

            let parsed: DeeplResponse =
                serde_json::from_str(&response_text).with_context(|| {
                    format!("DeepL JSON parse (body was: {} bytes)", response_text.len())
                })?;
            let out = parsed
                .translations
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("DeepL returned no translations"))?
                .text;
            Ok(out)
        })
    }
}
