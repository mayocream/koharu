use std::future::Future;
use std::pin::Pin;

use serde::Serialize;

use koharu_http::http::http_client;

use crate::llm::{Language, prompt::system_prompt};

use super::{AnyProvider, ensure_provider_success};

pub struct OpenAiProvider {
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
}

impl AnyProvider for OpenAiProvider {
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
            };

            let response = http_client()
                .post("https://api.openai.com/v1/chat/completions")
                .bearer_auth(&self.api_key)
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&body)?)
                .send()
                .await?;

            let resp: serde_json::Value = ensure_provider_success("openai", response)
                .await?
                .json()
                .await?;

            let text = resp["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("OpenAI returned no content"))?
                .to_string();

            Ok(text)
        })
    }
}
