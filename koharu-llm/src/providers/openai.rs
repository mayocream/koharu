use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;

use crate::{Language, prompt::system_prompt};

use super::AnyProvider;
use super::chat_completions::{ChatCompletionsAuth, ChatCompletionsRequest, send_chat_completion};

pub struct OpenAiProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: String,
}

impl AnyProvider for OpenAiProvider {
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
                    provider: "openai",
                    endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
                    auth: ChatCompletionsAuth::Bearer(self.api_key.clone()),
                    model: model.to_string(),
                    system_prompt: system_prompt(target_language),
                    user_prompt: source.to_string(),
                    temperature: None,
                    max_tokens: None,
                },
            )
            .await
        })
    }
}
