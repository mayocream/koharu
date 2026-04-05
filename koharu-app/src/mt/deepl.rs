//! DeepL REST API (`/v2/translate`).

use anyhow::Context;
use koharu_core::DeeplTranslateOptions;
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;

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

/// Translate multiple segments in one request. Empty `texts` returns empty vec.
pub async fn translate_batch(
    client: &ClientWithMiddleware,
    api_key: &str,
    base_url: Option<&str>,
    texts: &[String],
    target_lang: &str,
    source_lang: Option<&str>,
    options: Option<&DeeplTranslateOptions>,
) -> anyhow::Result<Vec<String>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let root = normalize_base_url(base_url);
    let url = format!("{root}/v2/translate");

    let encoded = {
        let mut ser = url::form_urlencoded::Serializer::new(String::new());
        for t in texts {
            ser.append_pair("text", t);
        }
        ser.append_pair("target_lang", target_lang);
        if let Some(s) = source_lang {
            ser.append_pair("source_lang", s);
        }
        if let Some(opts) = options {
            if let Some(f) = opts
                .formality
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                ser.append_pair("formality", f);
            }
            if let Some(m) = opts
                .model_type
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                ser.append_pair("model_type", m);
            }
        }
        ser.finish()
    };

    let response = client
        .post(&url)
        .header("Authorization", format!("DeepL-Auth-Key {api_key}"))
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

    let parsed: DeeplResponse = serde_json::from_str(&response_text)
        .with_context(|| format!("DeepL JSON parse (body was: {} bytes)", response_text.len()))?;

    if parsed.translations.len() != texts.len() {
        anyhow::bail!(
            "DeepL returned {} segments, expected {}",
            parsed.translations.len(),
            texts.len()
        );
    }

    Ok(parsed.translations.into_iter().map(|t| t.text).collect())
}
