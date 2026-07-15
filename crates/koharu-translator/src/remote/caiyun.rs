// Ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/caiyun.rs

use anyhow::{Context, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{ApiKey, send_json};
use crate::{Error, Language, Result, TranslationRequest};

const URL: &str = "https://api.interpreter.caiyunai.com/v1/translator";

#[derive(Debug, Clone)]
pub struct CaiyunConfig {
    pub api_key: ApiKey,
}

impl CaiyunConfig {
    #[must_use]
    pub fn new(api_key: impl Into<ApiKey>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }
}

pub(super) async fn translate(
    client: &Client,
    config: &CaiyunConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let target = target(request.target_language).ok_or(Error::UnsupportedLanguage {
        provider: "caiyun",
        language: request.target_language,
    })?;
    let response: Response = send_json(
        "caiyun",
        client
            .post(URL)
            .header(
                "X-Authorization",
                format!("token {}", config.api_key.expose()),
            )
            .json(&Request {
                source: &request.segments,
                trans_type: format!("auto2{target}"),
                request_id: "koharu-translator",
                detect: true,
                media: "text",
            }),
    )
    .await?;
    if response.rc != 0 {
        return Err(anyhow!(
            "Caiyun returned rc={}: {}",
            response.rc,
            response
                .msg
                .or(response.message)
                .or(response.error)
                .unwrap_or_else(|| "unknown error".to_owned())
        )
        .into());
    }
    Ok(
        match response.target.context("Caiyun returned no target")? {
            Target::One(text) => vec![text],
            Target::Many(texts) => texts,
        },
    )
}

#[derive(Serialize)]
struct Request<'a> {
    source: &'a [String],
    trans_type: String,
    request_id: &'static str,
    detect: bool,
    media: &'static str,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Target {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    rc: i64,
    target: Option<Target>,
    msg: Option<String>,
    message: Option<String>,
    error: Option<String>,
}

fn target(language: Language) -> Option<&'static str> {
    use Language::*;
    Some(match language {
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
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_languages_are_not_substituted() {
        assert_eq!(target(Language::BrazilianPortuguese), None);
        assert_eq!(target(Language::Japanese), Some("ja"));
    }
}
