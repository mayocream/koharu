use serde::Serialize;
use serde_json::Value;

use eventsource_stream::Eventsource;
use futures::StreamExt;

use super::responses::{CodexInputContent, CodexInputItem};

const DEFAULT_IMAGE_INSTRUCTIONS: &str = "Generate or edit the requested image.";
const DEFAULT_IMAGE_QUALITY: &str = "high";

#[derive(Debug, Clone, Serialize)]
pub struct CodexImageGenerationRequest {
    pub model: String,
    pub instructions: String,
    pub tools: [CodexImageGenerationTool; 1],
    pub tool_choice: CodexImageToolChoice,
    pub input: Vec<CodexInputItem>,
    pub stream: bool,
    pub store: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexImageToolChoice {
    #[serde(rename = "type")]
    pub tool_type: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexImageGenerationTool {
    #[serde(rename = "type")]
    pub tool_type: &'static str,
    #[serde(flatten)]
    pub image_generation: CodexImageGenerationConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexImageGenerationConfig {
    pub quality: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexInputImage {
    pub url: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexImageStreamResult {
    pub image_url: Option<String>,
    pub response_text: Option<String>,
}

impl CodexImageGenerationRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            instructions: DEFAULT_IMAGE_INSTRUCTIONS.to_string(),
            tools: [CodexImageGenerationTool::default()],
            tool_choice: CodexImageToolChoice::image_generation(),
            input: vec![CodexInputItem::user_text(prompt)],
            stream: true,
            store: false,
        }
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }

    pub fn with_quality(mut self, quality: impl Into<String>) -> Self {
        self.tools[0].image_generation.quality = quality.into();
        self
    }

    pub fn with_size(mut self, size: impl Into<String>) -> Self {
        self.tools[0].image_generation.size = Some(size.into());
        self
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.tools[0].image_generation.action = Some(action.into());
        self
    }

    pub fn with_input_image(mut self, image: CodexInputImage) -> Self {
        let content = CodexInputContent::input_image_url(image.url, Some(image.detail));
        if let Some(item) = self.input.first_mut() {
            item.content.push(content);
        } else {
            self.input.push(CodexInputItem {
                item_type: "message",
                role: "user",
                content: vec![content],
            });
        }
        self
    }
}

impl CodexImageToolChoice {
    pub fn image_generation() -> Self {
        Self {
            tool_type: "image_generation",
        }
    }
}

impl CodexImageGenerationTool {
    pub fn new(image_generation: CodexImageGenerationConfig) -> Self {
        Self {
            tool_type: "image_generation",
            image_generation,
        }
    }
}

impl Default for CodexImageGenerationTool {
    fn default() -> Self {
        Self::new(CodexImageGenerationConfig::default())
    }
}

impl Default for CodexImageGenerationConfig {
    fn default() -> Self {
        Self {
            quality: DEFAULT_IMAGE_QUALITY.to_string(),
            size: None,
            action: None,
        }
    }
}

impl CodexInputImage {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            detail: "high".to_string(),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}

pub async fn image_response_stream_url(
    response: reqwest::Response,
) -> anyhow::Result<Option<String>> {
    Ok(image_response_stream_result(response).await?.image_url)
}

pub async fn image_response_stream_result(
    response: reqwest::Response,
) -> anyhow::Result<CodexImageStreamResult> {
    let mut stream = response.bytes_stream().eventsource();
    let mut collector = CodexImageStreamCollector::default();

    while let Some(event) = stream.next().await {
        let event = event?;
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            continue;
        };
        collector.push(&data)?;
    }

    Ok(collector.finish())
}

#[derive(Debug, Default)]
struct CodexImageStreamCollector {
    final_image: Option<String>,
    partial_image: Option<String>,
    output_text: Vec<String>,
    final_text: Option<String>,
}

impl CodexImageStreamCollector {
    fn push(&mut self, value: &Value) -> anyhow::Result<()> {
        if let Some(error) = extract_response_error(value) {
            anyhow::bail!("{error}");
        }

        if let Some(url) = extract_final_image_url(value) {
            self.final_image = Some(url);
        }
        if let Some(url) = extract_partial_image_url(value) {
            self.partial_image = Some(url);
        }
        if let Some(delta) = extract_output_text_delta(value) {
            self.output_text.push(delta);
        }
        if let Some(text) = extract_response_text(value) {
            self.final_text = Some(text);
        }

        Ok(())
    }

    fn finish(self) -> CodexImageStreamResult {
        CodexImageStreamResult {
            image_url: self.final_image.or(self.partial_image),
            response_text: self
                .final_text
                .or_else(|| join_text_fragments(self.output_text)),
        }
    }
}

pub fn extract_image_url(value: &Value) -> Option<String> {
    extract_final_image_url(value).or_else(|| extract_partial_image_url(value))
}

fn extract_final_image_url(value: &Value) -> Option<String> {
    find_map_value(value, &mut |value| {
        let Value::Object(map) = value else {
            return None;
        };

        if matches!(
            map.get("type").and_then(Value::as_str),
            Some("image_generation_call")
        ) {
            return map.get("result").and_then(extract_image_result);
        }

        map.get("image_generation_call")
            .and_then(extract_final_image_url)
            .or_else(|| {
                map.get("url")
                    .or_else(|| map.get("image_url"))
                    .and_then(Value::as_str)
                    .filter(|url| is_image_url(url))
                    .map(ToOwned::to_owned)
            })
    })
}

fn extract_partial_image_url(value: &Value) -> Option<String> {
    let Value::Object(map) = value else {
        return None;
    };

    let b64 = map
        .get("partial_image_b64")
        .or_else(|| map.get("b64_json"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())?;
    Some(format!("data:image/png;base64,{b64}"))
}

fn extract_image_result(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if value.starts_with("http://") || value.starts_with("https://") => {
            Some(value.clone())
        }
        Value::String(value) if value.starts_with("data:image/") => Some(value.clone()),
        Value::String(value) if !value.is_empty() => Some(format!("data:image/png;base64,{value}")),
        Value::Object(map) => map
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| is_image_url(url))
            .map(ToOwned::to_owned)
            .or_else(|| map.values().find_map(extract_final_image_url)),
        Value::Array(items) => items.iter().find_map(extract_final_image_url),
        _ => None,
    }
}

fn is_image_url(value: &str) -> bool {
    value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("data:image/")
}

fn extract_output_text_delta(value: &Value) -> Option<String> {
    let Value::Object(map) = value else {
        return None;
    };
    if !matches!(
        map.get("type").and_then(Value::as_str),
        Some("response.output_text.delta")
    ) {
        return None;
    }
    map.get("delta")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_response_text(value: &Value) -> Option<String> {
    let mut fragments = Vec::new();
    collect_response_text(value, &mut fragments);
    join_text_fragments(fragments)
}

fn collect_response_text(value: &Value, fragments: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if matches!(
                map.get("type").and_then(Value::as_str),
                Some("response.output_text.done" | "output_text")
            ) && let Some(text) = map.get("text").and_then(Value::as_str)
                && !text.is_empty()
            {
                fragments.push(text.to_string());
                return;
            }

            if matches!(map.get("type").and_then(Value::as_str), Some("message"))
                && !matches!(map.get("role").and_then(Value::as_str), Some("assistant"))
            {
                return;
            }

            for key in ["response", "output", "item", "content"] {
                if let Some(child) = map.get(key) {
                    collect_response_text(child, fragments);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_response_text(item, fragments);
            }
        }
        _ => {}
    }
}

fn join_text_fragments(fragments: Vec<String>) -> Option<String> {
    let text = fragments.concat();
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn find_map_value(value: &Value, f: &mut impl FnMut(&Value) -> Option<String>) -> Option<String> {
    if let Some(found) = f(value) {
        return Some(found);
    }

    match value {
        Value::Object(map) => map.values().find_map(|child| find_map_value(child, f)),
        Value::Array(items) => items.iter().find_map(|child| find_map_value(child, f)),
        _ => None,
    }
}

fn extract_response_error(value: &Value) -> Option<String> {
    let Value::Object(map) = value else {
        return None;
    };
    let event_type = map.get("type").and_then(Value::as_str);
    if !matches!(
        event_type,
        Some("response.failed" | "response.incomplete" | "error")
    ) {
        return None;
    }

    map.get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .or_else(|| map.get("message").and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_image_generation_request_without_input_image() {
        let request = CodexImageGenerationRequest::new("gpt-image-2", "draw a koharu logo")
            .with_action("generate");
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["model"], "gpt-image-2");
        assert_eq!(value["instructions"], DEFAULT_IMAGE_INSTRUCTIONS);
        assert!(value["input"].is_array());
        assert_eq!(value["input"][0]["type"], "message");
        assert_eq!(value["input"][0]["role"], "user");
        assert_eq!(value["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(
            value["input"][0]["content"][0]["text"],
            "draw a koharu logo"
        );
        assert_eq!(value["tools"][0]["type"], "image_generation");
        assert_eq!(value["tool_choice"]["type"], "image_generation");
        assert_eq!(value["tools"][0]["quality"], "high");
        assert_eq!(value["tools"][0]["action"], "generate");
        assert_eq!(value["stream"], true);
        assert_eq!(value["store"], false);
        assert!(value.get("input_image").is_none());
    }

    #[test]
    fn serializes_image_generation_request_with_input_image() {
        let request = CodexImageGenerationRequest::new("gpt-image-2", "make it manga")
            .with_action("edit")
            .with_input_image(
                CodexInputImage::new("data:image/png;base64,abc").with_detail("high"),
            );
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["input"][0]["content"][1]["type"], "input_image");
        assert_eq!(
            value["input"][0]["content"][1]["image_url"],
            "data:image/png;base64,abc"
        );
        assert_eq!(value["input"][0]["content"][1]["detail"], "high");
        assert_eq!(value["tools"][0]["action"], "edit");
        assert_eq!(value["tool_choice"]["type"], "image_generation");
        assert!(value.get("input_image").is_none());
    }

    #[test]
    fn extracts_nested_image_generation_url() {
        let value = serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "type": "image_generation_call",
                "result": {
                    "url": "https://example.test/image.png"
                }
            }
        });

        assert_eq!(
            extract_image_url(&value),
            Some("https://example.test/image.png".to_string())
        );
    }

    #[test]
    fn converts_base64_image_generation_result_to_data_url() {
        let value = serde_json::json!({
            "type": "image_generation_call",
            "result": "abc123"
        });

        assert_eq!(
            extract_image_url(&value),
            Some("data:image/png;base64,abc123".to_string())
        );
    }

    #[test]
    fn extracts_responses_partial_image_event() {
        let value = serde_json::json!({
            "type": "response.image_generation_call.partial_image",
            "partial_image_b64": "abc123"
        });

        assert_eq!(
            extract_image_url(&value),
            Some("data:image/png;base64,abc123".to_string())
        );
    }

    #[test]
    fn extracts_images_stream_completed_event() {
        let value = serde_json::json!({
            "type": "image_generation.completed",
            "b64_json": "def456"
        });

        assert_eq!(
            extract_image_url(&value),
            Some("data:image/png;base64,def456".to_string())
        );
    }

    #[test]
    fn extracts_stream_error_message() {
        let value = serde_json::json!({
            "type": "response.failed",
            "error": { "message": "image generation failed" }
        });

        assert_eq!(
            extract_response_error(&value),
            Some("image generation failed".to_string())
        );
    }

    #[test]
    fn extracts_response_text_from_completed_message() {
        let value = serde_json::json!({
            "type": "response.completed",
            "response": {
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "I could not generate the image."
                            }
                        ]
                    }
                ]
            }
        });

        assert_eq!(
            extract_response_text(&value),
            Some("I could not generate the image.".to_string())
        );
    }

    #[test]
    fn collector_prefers_final_image_and_keeps_text() {
        let mut collector = CodexImageStreamCollector::default();
        collector
            .push(&serde_json::json!({
                "type": "response.output_text.delta",
                "delta": "Working"
            }))
            .unwrap();
        collector
            .push(&serde_json::json!({
                "type": "response.image_generation_call.partial_image",
                "partial_image_b64": "partial"
            }))
            .unwrap();
        collector
            .push(&serde_json::json!({
                "type": "image_generation_call",
                "result": "final"
            }))
            .unwrap();

        assert_eq!(
            collector.finish(),
            CodexImageStreamResult {
                image_url: Some("data:image/png;base64,final".to_string()),
                response_text: Some("Working".to_string()),
            }
        );
    }
}
