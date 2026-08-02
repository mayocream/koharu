use serde::Serialize;
use serde_json::{Value, json};

use crate::{Reasoning, Tool};

#[derive(Debug, Serialize)]
pub(crate) struct Request {
    pub(super) model: String,
    pub(super) instructions: String,
    pub(super) input: Vec<Value>,
    pub(super) tools: Vec<Tool>,
    pub(super) tool_choice: &'static str,
    pub(super) parallel_tool_calls: bool,
    pub(super) reasoning: ReasoningOptions,
    pub(super) text: TextOptions,
    pub(super) include: [&'static str; 1],
    pub(super) stream: bool,
    pub(super) store: bool,
    pub(super) prompt_cache_key: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ReasoningOptions {
    effort: &'static str,
    summary: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct TextOptions {
    verbosity: &'static str,
}

impl Request {
    pub(crate) fn new(
        model: String,
        instructions: String,
        input: Vec<Value>,
        tools: Vec<Tool>,
        reasoning: Reasoning,
        session: String,
    ) -> Self {
        Self {
            model,
            instructions,
            input,
            tools,
            tool_choice: "auto",
            parallel_tool_calls: false,
            reasoning: ReasoningOptions {
                effort: reasoning.as_str(),
                summary: "auto",
            },
            text: TextOptions { verbosity: "low" },
            include: ["reasoning.encrypted_content"],
            stream: true,
            store: false,
            prompt_cache_key: session,
        }
    }
}

pub(crate) fn message(role: &str, text: impl Into<String>) -> Value {
    json!({
        "type": "message",
        "role": role,
        "content": [{
            "type": "input_text",
            "text": text.into(),
        }],
    })
}

pub(crate) fn project_context(data: &Value) -> Result<Value, serde_json::Error> {
    let content = vec![json!({
        "type": "input_text",
        "text": format!(
            "<koharu_project_context>\n{}\n</koharu_project_context>",
            serde_json::to_string(data)?
        ),
    })];
    Ok(json!({
        "type": "message",
        "role": "user",
        "content": content,
    }))
}

pub(crate) fn function_output(
    call_id: &str,
    output: &Value,
    images: &[crate::ToolImage],
) -> Result<Value, serde_json::Error> {
    let output = if images.is_empty() {
        Value::String(serde_json::to_string(output)?)
    } else {
        let mut content = vec![json!({
            "type": "input_text",
            "text": serde_json::to_string(output)?,
        })];
        for image in images {
            content.push(json!({
                "type": "input_text",
                "text": image.label,
            }));
            content.push(json!({
                "type": "input_image",
                "image_url": image.data_url,
                "detail": "high",
            }));
        }
        Value::Array(content)
    };
    Ok(json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": output,
    }))
}
