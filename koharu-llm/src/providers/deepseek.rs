use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

use koharu_runtime::http::http_client;

use crate::{Language, prompt::system_prompt};

use super::{AnyProvider, ensure_provider_success};

pub struct DeepSeekProvider {
    pub api_key: SecretString,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[async_trait]
impl AnyProvider for DeepSeekProvider {
    async fn translate(
        &self,
        source: &str,
        target_language: Language,
        model: &str,
    ) -> anyhow::Result<String> {
        let body = ChatRequest {
            model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: system_prompt(target_language),
                },
                ChatMessage {
                    role: "user",
                    content: source.to_string(),
                },
            ],
            temperature: 1.3,
        };

        let response = http_client()
            .post("https://api.deepseek.com/chat/completions")
            .bearer_auth(self.api_key.expose_secret())
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?;

        let resp: serde_json::Value = ensure_provider_success("deepseek", response)
            .await?
            .json()
            .await?;

        let text = resp["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("DeepSeek returned no content"))?
            .to_string();

        Ok(text)
    }
}
