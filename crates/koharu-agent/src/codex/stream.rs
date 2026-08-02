use std::collections::HashSet;

use anyhow::{Context as _, Result, anyhow, bail};
use eventsource_stream::Eventsource as _;
use futures::StreamExt as _;
use serde_json::Value;

use crate::{Control, ToolCall};

const MAX_STREAM_BYTES: usize = 16 * 1024 * 1024;

pub(crate) enum Delta {
    Text(String),
    Reasoning(String),
}

pub(crate) struct Turn {
    pub(crate) output: Vec<Value>,
    pub(crate) calls: Vec<ToolCall>,
    pub(crate) text: String,
}

pub(super) async fn read<F>(
    response: reqwest::Response,
    control: &Control,
    mut publish: F,
) -> Result<Turn>
where
    F: FnMut(Delta),
{
    let mut stream = response.bytes_stream().eventsource();
    let mut bytes = 0_usize;
    let mut output = Vec::new();
    let mut text = String::new();
    let mut completed = false;

    loop {
        let event = tokio::select! {
            event = stream.next() => event,
            () = control.cancelled() => {
                control.ensure_running()?;
                unreachable!("cancelled control must fail ensure_running")
            }
        };
        let Some(event) = event else {
            break;
        };
        let event = event.map_err(|error| anyhow!("invalid Codex event stream: {error}"))?;
        bytes = bytes.saturating_add(event.data.len());
        if bytes > MAX_STREAM_BYTES {
            bail!("Codex response stream exceeded {MAX_STREAM_BYTES} bytes");
        }
        if event.data == "[DONE]" || event.data.trim().is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(&event.data).context("invalid JSON in Codex response stream")?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "response.output_text.delta" => {
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    text.push_str(delta);
                    publish(Delta::Text(delta.to_owned()));
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    publish(Delta::Reasoning(delta.to_owned()));
                }
            }
            "response.output_item.done" => {
                if let Some(item) = value.get("item") {
                    output.push(item.clone());
                }
            }
            "response.completed" | "response.done" => {
                if output.is_empty()
                    && let Some(items) = value
                        .get("response")
                        .and_then(|response| response.get("output"))
                        .and_then(Value::as_array)
                {
                    output.extend(items.iter().cloned());
                }
                completed = true;
                break;
            }
            "response.incomplete" => {
                bail!("Codex response was incomplete: {}", provider_error(&value));
            }
            "response.failed" => {
                bail!("Codex response failed: {}", provider_error(&value));
            }
            "error" => bail!("Codex returned an error: {}", provider_error(&value)),
            _ => {}
        }
    }
    if !completed {
        bail!("Codex response stream ended before completion");
    }

    if text.is_empty() {
        text = output_text(&output);
        if !text.is_empty() {
            publish(Delta::Text(text.clone()));
        }
    }
    let mut seen = HashSet::new();
    let calls = output
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .filter_map(|item| {
            let call_id = item.get("call_id")?.as_str()?.to_owned();
            if !seen.insert(call_id.clone()) {
                return None;
            }
            Some(ToolCall {
                call_id,
                name: item.get("name")?.as_str()?.to_owned(),
                arguments: item.get("arguments")?.as_str()?.to_owned(),
            })
        })
        .collect();
    Ok(Turn {
        output,
        calls,
        text,
    })
}

fn output_text(output: &[Value]) -> String {
    output
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .filter_map(|item| item.get("content")?.as_array())
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|content| content.get("text")?.as_str())
        .collect::<String>()
}

fn provider_error(value: &Value) -> String {
    value
        .get("message")
        .or_else(|| value.get("error").and_then(|error| error.get("message")))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}
