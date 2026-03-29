use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

use koharu_runtime::http::http_client;

use crate::{Language, prompt::system_prompt};

use super::{AnyProvider, ensure_provider_success};

pub struct ClaudeProvider {
    pub api_key: SecretString,
}

#[derive(Serialize)]
struct UserMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: String,
    messages: Vec<UserMessage>,
}

#[async_trait]
impl AnyProvider for ClaudeProvider {
    async fn translate(
        &self,
        source: &str,
        target_language: Language,
        model: &str,
    ) -> anyhow::Result<String> {
        let body = MessagesRequest {
            model,
            max_tokens: 8192,
            system: system_prompt(target_language),
            messages: vec![UserMessage {
                role: "user",
                content: source.to_string(),
            }],
        };

        let response = http_client()
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;

        let resp: serde_json::Value = ensure_provider_success("claude", response)
            .await?
            .json()
            .await?;

        let text = resp["content"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Claude returned no content"))?
            .to_string();

        Ok(text)
    }
}
