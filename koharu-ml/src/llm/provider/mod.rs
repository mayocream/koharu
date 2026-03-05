use std::future::Future;
use std::pin::Pin;

pub mod claude;
pub mod gemini;
pub mod openai;

pub const SYSTEM_PROMPT_TEMPLATE: &str =
    "You are a professional manga/comic translator. \
     Translate the following text to {target_language}. \
     Preserve line breaks. Output only the translation, no explanations.";

pub fn system_prompt(target_language: &str) -> String {
    SYSTEM_PROMPT_TEMPLATE.replace("{target_language}", target_language)
}

pub trait AnyProvider: Send + Sync {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: &'a str,
        model: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>>;
}
