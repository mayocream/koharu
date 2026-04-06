//! Google Cloud Translation API v2 (`language.translate`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Context;
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};

use crate::Language;

use super::AnyProvider;

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

pub struct GoogleTranslateMtProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: String,
}

impl AnyProvider for GoogleTranslateMtProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        _model: &'a str,
        _custom_system_prompt: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let body = GoogleRequest {
                q: vec![source],
                target: target_language.tag(),
                source: None,
                format: Some("text"),
            };

            let url = format!("{GOOGLE_TRANSLATE_URL}?key={}", self.api_key);
            let json =
                serde_json::to_vec(&body).context("serialize Google Translate request body")?;

            let response = self
                .http_client
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
            let out = parsed
                .data
                .translations
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Google returned no translations"))?
                .translated_text;
            Ok(out)
        })
    }
}
