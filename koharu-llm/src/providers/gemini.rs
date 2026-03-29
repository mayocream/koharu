use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

use koharu_runtime::http::http_client;

use crate::{Language, prompt::system_prompt};

use super::{AnyProvider, ensure_provider_success};

pub struct GeminiProvider {
    pub api_key: SecretString,
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

#[async_trait]
impl AnyProvider for GeminiProvider {
    async fn translate(
        &self,
        source: &str,
        target_language: Language,
        model: &str,
    ) -> anyhow::Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            model,
            self.api_key.expose_secret()
        );

        let body = GenerateRequest {
            system_instruction: SystemInstruction {
                parts: vec![Part {
                    text: system_prompt(target_language),
                }],
            },
            contents: vec![Content {
                parts: vec![Part {
                    text: source.to_string(),
                }],
            }],
        };

        let response = http_client()
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
    }
}
