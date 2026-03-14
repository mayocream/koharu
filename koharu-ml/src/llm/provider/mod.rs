use std::future::Future;
use std::pin::Pin;

use anyhow::Context;

pub mod claude;
pub mod gemini;
pub mod openai;

pub const SYSTEM_PROMPT_TEMPLATE: &str = "You are a professional manga/comic translator. \
     Translate the following text to {target_language}. \
     Preserve line breaks. Output only the translation, no explanations.";

pub fn system_prompt(target_language: &str) -> String {
    SYSTEM_PROMPT_TEMPLATE.replace("{target_language}", target_language)
}

pub async fn ensure_provider_success(
    provider: &str,
    response: reqwest::Response,
) -> anyhow::Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response
        .text()
        .await
        .with_context(|| format!("Failed to read {provider} error response body"))?;
    let body_lower = body.to_ascii_lowercase();
    let quota_exceeded = status.as_u16() == 429
        || body_lower.contains("insufficient_quota")
        || body_lower.contains("quota")
        || body_lower.contains("resource_exhausted")
        || body_lower.contains("rate limit exceeded")
        || body_lower.contains("credit balance is too low");

    if quota_exceeded {
        anyhow::bail!("provider_quota_exceeded:{provider}");
    }

    anyhow::bail!("{provider} API request failed ({status}): {body}");
}

pub trait AnyProvider: Send + Sync {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: &'a str,
        model: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>>;
}
