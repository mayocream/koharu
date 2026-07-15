// Ported from:
// https://github.com/mayocream/koharu/blob/f4ce03999ed1ae2faaec938dd52c2f41a87d03d9/crates/koharu-llm/src/providers/google_translate.rs

use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use super::{ApiKey, send_json};
use crate::{Result, TranslationRequest};

const URL: &str = "https://translation.googleapis.com/language/translate/v2";

#[derive(Debug, Clone)]
pub struct GoogleCloudConfig {
    pub api_key: ApiKey,
}

impl GoogleCloudConfig {
    #[must_use]
    pub fn new(api_key: impl Into<ApiKey>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }
}

pub(super) async fn translate(
    client: &Client,
    config: &GoogleCloudConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let mut url = Url::parse(URL).expect("Google API URL is valid");
    url.query_pairs_mut()
        .append_pair("key", config.api_key.expose());
    let response: Response = send_json(
        "google-cloud-translation",
        client.post(url).json(&Request {
            q: &request.segments,
            target: request.target_language.tag(),
            source: request.source_language.map(|language| language.tag()),
            format: "text",
        }),
    )
    .await?;
    Ok(response
        .data
        .translations
        .into_iter()
        .map(|translation| translation.translated_text)
        .collect())
}

#[derive(Serialize)]
struct Request<'a> {
    q: &'a [String],
    target: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a str>,
    format: &'static str,
}

#[derive(Deserialize)]
struct Response {
    data: Data,
}

#[derive(Deserialize)]
struct Data {
    translations: Vec<Translation>,
}

#[derive(Deserialize)]
struct Translation {
    #[serde(rename = "translatedText")]
    translated_text: String,
}
