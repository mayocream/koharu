use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;
use serde::Serialize;

use crate::Language;

use super::{AnyProvider, TranslateOptions, ensure_provider_success, resolve_system_prompt};

pub struct GeminiProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: String,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct SystemInstruction {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct GenerateRequest {
    system_instruction: SystemInstruction,
    contents: Vec<Content>,
}

impl AnyProvider for GeminiProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        model: &'a str,
        custom_system_prompt: Option<&'a str>,
        _options: Option<&'a TranslateOptions>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                model, self.api_key
            );

            let body = GenerateRequest {
                system_instruction: SystemInstruction {
                    parts: vec![Part {
                        text: resolve_system_prompt(custom_system_prompt, target_language),
                    }],
                },
                contents: vec![Content {
                    parts: vec![Part {
                        text: source.to_string(),
                    }],
                }],
            };

            let response = self
                .http_client
                .post(&url)
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&body)?)
                .send()
                .await?;

            let resp: serde_json::Value = ensure_provider_success("gemini", response)
                .await?
                .json()
                .await?;

            let text = resp["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Gemini returned no content"))?
                .to_string();

            Ok(text)
        })
    }
}
