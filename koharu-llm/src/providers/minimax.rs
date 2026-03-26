use std::future::Future;
use std::pin::Pin;

use serde::Serialize;

use koharu_http::http::http_client;

use crate::{Language, prompt::system_prompt};

use super::{AnyProvider, ensure_provider_success};

pub struct MiniMaxProvider {
    pub api_key: String,
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

impl AnyProvider for MiniMaxProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        model: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
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
                temperature: 0.7,
            };

            let response = http_client()
                .post("https://api.minimax.io/v1/chat/completions")
                .bearer_auth(&self.api_key)
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&body)?)
                .send()
                .await?;

            let resp: serde_json::Value =
                ensure_provider_success("minimax", response)
                    .await?
                    .json()
                    .await?;

            let text = resp["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("MiniMax returned no content"))?
                .to_string();

            Ok(text)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_request_serializes_correctly() {
        let req = ChatRequest {
            model: "MiniMax-M2.7",
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: "You are a translator.".to_string(),
                },
                ChatMessage {
                    role: "user",
                    content: "こんにちは".to_string(),
                },
            ],
            temperature: 0.7,
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "MiniMax-M2.7");
        assert_eq!(json["temperature"], 0.7);
        assert_eq!(json["messages"].as_array().unwrap().len(), 2);
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(json["messages"][1]["role"], "user");
        assert_eq!(json["messages"][1]["content"], "こんにちは");
    }

    #[test]
    fn temperature_is_within_valid_range() {
        let req = ChatRequest {
            model: "MiniMax-M2.7",
            messages: vec![],
            temperature: 0.7,
        };
        assert!(req.temperature > 0.0 && req.temperature <= 1.0);
    }

    #[test]
    fn provider_struct_holds_api_key() {
        let provider = MiniMaxProvider {
            api_key: "test-key-123".to_string(),
        };
        assert_eq!(provider.api_key, "test-key-123");
    }
}
