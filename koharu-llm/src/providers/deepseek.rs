use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;

use crate::Language;

use super::AnyProvider;
use super::TranslationOutcome;
use super::chat_completions::{
    ChatCompletionsAuth, ChatCompletionsRequest, send_chat_completion, send_chat_completion_outcome,
};
use super::chat_finish_reason;
use super::resolve_system_prompt;

pub struct DeepSeekProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: String,
}

impl DeepSeekProvider {
    fn chat_request(
        &self,
        source: &str,
        target_language: Language,
        model: &str,
        custom_system_prompt: Option<&str>,
    ) -> ChatCompletionsRequest {
        ChatCompletionsRequest {
            provider: "deepseek",
            endpoint: "https://api.deepseek.com/chat/completions".to_string(),
            auth: ChatCompletionsAuth::Bearer(self.api_key.clone()),
            model: model.to_string(),
            system_prompt: resolve_system_prompt(custom_system_prompt, target_language),
            user_prompt: source.to_string(),
            temperature: Some(1.3),
            max_tokens: None,
        }
    }
}

impl AnyProvider for DeepSeekProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        model: &'a str,
        custom_system_prompt: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            send_chat_completion(
                Arc::clone(&self.http_client),
                self.chat_request(source, target_language, model, custom_system_prompt),
            )
            .await
        })
    }

    fn translate_with_outcome<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        model: &'a str,
        custom_system_prompt: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<TranslationOutcome>> + Send + 'a>> {
        Box::pin(async move {
            let outcome = send_chat_completion_outcome(
                Arc::clone(&self.http_client),
                self.chat_request(source, target_language, model, custom_system_prompt),
            )
            .await?;
            Ok(TranslationOutcome {
                text: outcome.text,
                finish_reason: chat_finish_reason(outcome.finish_reason),
            })
        })
    }
}
