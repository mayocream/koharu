use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;
use serde::Serialize;

use crate::Language;

use super::{AnyProvider, ensure_provider_success, resolve_system_prompt};

pub struct GeminiProvider {
    pub http_client: Arc<ClientWithMiddleware>,
    pub api_key: String,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct SystemInstruction {
    parts: Vec<Part>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateRequest {
    system_instruction: SystemInstruction,
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

impl AnyProvider for GeminiProvider {
    fn translate<'a>(
        &'a self,
        source: &'a str,
        target_language: Language,
        model: &'a str,
        custom_system_prompt: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                model, self.api_key
            );

            // Use provided config or defaults for Gemma 4 if not specified
            let (temp, top_p, top_k) = if model.starts_with("gemma-4") {
                (
                    self.temperature.or(Some(1.0)),
                    self.top_p.or(Some(0.95)),
                    self.top_k.or(Some(64)),
                )
            } else {
                (self.temperature, self.top_p, self.top_k)
            };

            let body = GenerateRequest {
                system_instruction: SystemInstruction {
                    parts: vec![Part {
                        text: resolve_system_prompt(custom_system_prompt, target_language),
                    }],
                },
                contents: vec![Content {
                    parts: vec![Part {
                        text: source.to_string(),
                    }],
                }],
                generation_config: Some(GenerationConfig {
                    temperature: temp,
                    top_p,
                    top_k,
                }),
            };

            let response = self
                .http_client
                .post(&url)
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&body)?)
                .send()
                .await?;

            let resp: serde_json::Value = ensure_provider_success("gemini", response)
                .await?
                .json()
                .await?;

            let parts = resp["candidates"][0]["content"]["parts"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Gemini returned no parts"))?;

            // Skip thought blocks (reasoning)
            let text = parts
                .iter()
                .filter(|p| p["thought"].as_bool() != Some(true))
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("");

            if text.is_empty() {
                anyhow::bail!("Gemini returned no content after filtering thoughts");
            }

            Ok(text)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn test_filter_thoughts() {
        let resp = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {
                            "text": "Thinking about the translation...",
                            "thought": true
                        },
                        {
                            "text": "Hello world"
                        }
                    ]
                }
            }]
        });

        let parts = resp["candidates"][0]["content"]["parts"]
            .as_array()
            .unwrap();
        let text = parts
            .iter()
            .filter(|p| p["thought"].as_bool() != Some(true))
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("");

        assert_eq!(text, "Hello world");
    }
}
