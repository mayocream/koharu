//! Google Cloud Translation API v2 (`language.translate`).

use anyhow::Context;
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};

const GOOGLE_TRANSLATE_URL: &str = "https://translation.googleapis.com/language/translate/v2";

#[derive(Debug, Serialize)]
struct GoogleRequest<'a> {
    q: Vec<&'a str>,
    target: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'static str>,
}

#[derive(Debug, Deserialize)]
struct GoogleResponse {
    data: GoogleData,
}

#[derive(Debug, Deserialize)]
struct GoogleData {
    translations: Vec<GoogleTranslation>,
}

#[derive(Debug, Deserialize)]
struct GoogleTranslation {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

/// Translate multiple segments in one request. Empty `texts` returns empty vec.
pub async fn translate_batch(
    client: &ClientWithMiddleware,
    api_key: &str,
    texts: &[String],
    target: &str,
    source: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let q: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let body = GoogleRequest {
        q,
        target,
        source,
        format: Some("text"),
    };

    let url = format!("{GOOGLE_TRANSLATE_URL}?key={api_key}");

    let json = serde_json::to_vec(&body).context("serialize Google Translate request body")?;

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(json)
        .send()
        .await
        .context("Google Translate request")?;

    let status = response.status();
    let response_text = response.text().await.context("Google response body")?;

    if !status.is_success() {
        anyhow::bail!("Google Translate API failed ({status}): {response_text}");
    }

    let parsed: GoogleResponse = serde_json::from_str(&response_text)
        .with_context(|| format!("Google JSON parse: {response_text}"))?;

    if parsed.data.translations.len() != texts.len() {
        anyhow::bail!(
            "Google returned {} segments, expected {}",
            parsed.data.translations.len(),
            texts.len()
        );
    }

    Ok(parsed
        .data
        .translations
        .into_iter()
        .map(|t| t.translated_text)
        .collect())
}
