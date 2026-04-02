use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;

use crate::{Language, prompt::system_prompt};

use super::AnyProvider;
use super::chat_completions::{ChatCompletionsAuth, ChatCompletionsRequest, send_chat_completion};

pub struct DeepSeekProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: String,
}

impl AnyProvider for DeepSeekProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        model: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            send_chat_completion(
                Arc::clone(&self.http_client),
                ChatCompletionsRequest {
                    provider: "deepseek",
                    endpoint: "https://api.deepseek.com/chat/completions".to_string(),
                    auth: ChatCompletionsAuth::Bearer(self.api_key.clone()),
                    model: model.to_string(),
                    system_prompt: system_prompt(target_language),
                    user_prompt: source.to_string(),
                    temperature: Some(1.3),
                    max_tokens: None,
                },
            )
            .await
        })
    }
}
