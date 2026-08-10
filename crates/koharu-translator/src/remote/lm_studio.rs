// https://lmstudio.ai/docs/developer/rest/chat
// https://lmstudio.ai/docs/developer/rest/list

use anyhow::Context;
use koharu_secrets::ExposeSecret;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

use super::send_json;
use crate::{
    GenerationConfig, Model, Provider, Result, TranslationRequest, backend::encode_image,
    display_name, prompt,
};

const DEFAULT_BASE_URL: &str = "http://localhost:1234";

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct LmStudioConfig {
    pub base_url: Option<Url>,
}

impl Default for LmStudioConfig {
    fn default() -> Self {
        Self {
            base_url: Some(Url::parse(DEFAULT_BASE_URL).expect("default LM Studio URL is valid")),
        }
    }
}

pub(super) async fn translate(
    client: &Client,
    config: &LmStudioConfig,
    model: &str,
    generation: &GenerationConfig,
    request: &TranslationRequest,
) -> Result<Vec<String>> {
    let api_key = koharu_secrets::get("lm-studio")?;
    let (system, input) = prompt::prompts(request)?;
    let body = ChatRequest {
        model,
        input: ChatInput::new(&input, request.image.as_deref())?,
        system_prompt: &system,
        temperature: generation.temperature,
        max_output_tokens: generation.max_tokens,
        reasoning: if generation.thinking { "on" } else { "off" },
        store: false,
    };
    let mut http = client.post(endpoint(config.base_url.as_ref(), "chat"));
    if let Some(api_key) = api_key {
        http = http.bearer_auth(api_key.expose_secret());
    }
    let response: ChatResponse = send_json("lm-studio", http.json(&body)).await?;
    let text = response
        .output
        .into_iter()
        .rev()
        .find_map(|output| {
            (output.kind == "message")
                .then_some(output.content)
                .flatten()
        })
        .context("LM Studio returned no message output")?;
    Ok(prompt::translations("lm-studio", &text, &request.segments)?)
}

pub(super) async fn models(client: &Client, config: &LmStudioConfig) -> Result<Vec<Model>> {
    let api_key = koharu_secrets::get("lm-studio")?;
    let mut request = client.get(endpoint(config.base_url.as_ref(), "models"));
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key.expose_secret());
    }
    let response: ModelsResponse = send_json("lm-studio", request).await?;
    Ok(response
        .models
        .into_iter()
        .filter(|model| matches!(model.kind.as_str(), "llm" | "vlm"))
        .map(|model| Model {
            provider: Provider::LmStudio,
            name: display_name(&model.key),
            model: Some(model.key),
            quantizations: Vec::new(),
            vision: model.kind == "vlm",
        })
        .collect())
}

fn endpoint(base_url: Option<&Url>, suffix: &str) -> String {
    let base_url = base_url.map_or(DEFAULT_BASE_URL, Url::as_str);
    format!(
        "{}/api/v1/{}",
        base_url.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    input: ChatInput<'a>,
    system_prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    reasoning: &'static str,
    store: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ChatInput<'a> {
    Text(&'a str),
    Items(Vec<InputItem<'a>>),
}

impl<'a> ChatInput<'a> {
    fn new(text: &'a str, image: Option<&image::DynamicImage>) -> anyhow::Result<Self> {
        let Some(image) = image else {
            return Ok(Self::Text(text));
        };
        Ok(Self::Items(vec![
            InputItem::Message { content: text },
            InputItem::Image {
                data_url: encode_image(image)?.data_url(),
            },
        ]))
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InputItem<'a> {
    Message { content: &'a str },
    Image { data_url: String },
}

#[derive(Deserialize)]
struct ChatResponse {
    output: Vec<Output>,
}

#[derive(Deserialize)]
struct Output {
    #[serde(rename = "type")]
    kind: String,
    content: Option<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    models: Vec<ListedModel>,
}

#[derive(Deserialize)]
struct ListedModel {
    #[serde(rename = "type")]
    kind: String,
    key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_native_v1_endpoints() {
        let base_url = Url::parse("http://localhost:1234/").unwrap();
        assert_eq!(
            endpoint(Some(&base_url), "chat"),
            "http://localhost:1234/api/v1/chat"
        );
        assert_eq!(
            endpoint(Some(&base_url), "models"),
            "http://localhost:1234/api/v1/models"
        );
    }

    #[test]
    fn serializes_native_chat_options() {
        let value = serde_json::to_value(ChatRequest {
            model: "publisher/model",
            input: ChatInput::Text("input"),
            system_prompt: "system",
            temperature: Some(0.2),
            max_output_tokens: Some(1024),
            reasoning: "off",
            store: false,
        })
        .unwrap();
        assert_eq!(value["max_output_tokens"], 1024);
        assert_eq!(value["reasoning"], "off");
        assert_eq!(value["store"], false);
        assert!(value.get("messages").is_none());
    }

    #[test]
    fn model_discovery_keeps_language_and_vision_models() {
        let response: ModelsResponse = serde_json::from_value(serde_json::json!({
            "models": [
                { "type": "llm", "key": "publisher/chat-model" },
                { "type": "vlm", "key": "publisher/vision-model" },
                { "type": "embedding", "key": "publisher/embed-model" }
            ]
        }))
        .unwrap();
        let models = response
            .models
            .into_iter()
            .filter_map(|model| matches!(model.kind.as_str(), "llm" | "vlm").then_some(model.key))
            .collect::<Vec<_>>();
        assert_eq!(models, ["publisher/chat-model", "publisher/vision-model"]);
    }

    #[test]
    fn serializes_native_image_input() {
        let input =
            ChatInput::new("translate", Some(&image::DynamicImage::new_rgb8(1, 1))).unwrap();
        let value = serde_json::to_value(input).unwrap();
        assert_eq!(
            value[0],
            serde_json::json!({ "type": "message", "content": "translate" })
        );
        assert_eq!(value[1]["type"], "image");
        assert!(
            value[1]["data_url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/jpeg;base64,")
        );
    }
}
