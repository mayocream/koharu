use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;

use crate::Language;

use super::AnyProvider;
use super::TranslateOptions;
use super::chat_completions::{ChatCompletionsAuth, ChatCompletionsRequest, send_chat_completion};
use super::resolve_system_prompt;

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
        custom_system_prompt: Option<&'a str>,
        _options: Option<&'a TranslateOptions>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            send_chat_completion(
                Arc::clone(&self.http_client),
                ChatCompletionsRequest {
                    provider: "openai",
                    endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
                    auth: ChatCompletionsAuth::Bearer(self.api_key.clone()),
                    model: model.to_string(),
                    system_prompt: resolve_system_prompt(custom_system_prompt, target_language),
                    user_prompt: source.to_string(),
                    temperature: None,
                    max_tokens: None,
                },
            )
            .await
        })
    }
}
