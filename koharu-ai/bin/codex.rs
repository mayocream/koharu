use std::io::Write as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use clap::{Parser, Subcommand};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use koharu_ai::codex::{
    CodexClient, CodexConfig, CodexImageGenerationRequest, CodexInputImage, CodexTaskRequest,
    DEFAULT_RESPONSES_URL,
};
use serde_json::Value;

#[derive(Debug, Parser)]
#[command(author, version, about = "Codex OAuth and task helper for Koharu")]
struct Args {
    #[command(subcommand)]
    command: Command,

    #[arg(long, global = true, default_value = DEFAULT_RESPONSES_URL)]
    responses_url: String,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start device-code login and store the OAuth token.
    Login,
    /// Print whether a stored token exists.
    Status,
    /// Delete the stored OAuth token.
    Logout,
    /// Call the Codex Responses API and print the JSON response.
    Run {
        /// Model name to send in the request body.
        #[arg(long)]
        model: String,
        /// Instruction string.
        #[arg(long)]
        instructions: String,
        /// Task text to send as input.
        input: String,
    },
    /// Generate or edit an image and print the generated image URL.
    Image {
        /// Image model name to send in the request body.
        #[arg(long, default_value = "gpt-5.5")]
        model: String,
        /// Instruction string.
        #[arg(long, default_value = "Generate or edit the requested image.")]
        instructions: String,
        /// Optional local input image path for image-to-image editing.
        #[arg(long)]
        image: Option<PathBuf>,
        /// Image generation quality.
        #[arg(long, default_value = "high")]
        quality: String,
        /// Optional size value accepted by the backend.
        #[arg(long)]
        size: Option<String>,
        /// Optional action. Defaults to edit when --image is set, otherwise generate.
        #[arg(long)]
        action: Option<String>,
        /// Input image detail.
        #[arg(long, default_value = "high")]
        detail: String,
        /// Image prompt.
        prompt: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = CodexClient::try_new(CodexConfig {
        responses_url: args.responses_url,
        ..CodexConfig::default()
    })?;

    match args.command {
        Command::Login => login(&client).await,
        Command::Status => status(&client),
        Command::Logout => logout(&client),
        Command::Run {
            model,
            instructions,
            input,
        } => run(&client, model, instructions, input).await,
        Command::Image {
            model,
            instructions,
            image: image_path,
            quality,
            size,
            action,
            detail,
            prompt,
        } => {
            image_cmd(
                &client,
                ImageCommand {
                    model,
                    instructions,
                    image: image_path,
                    quality,
                    size,
                    action,
                    detail,
                    prompt,
                },
            )
            .await
        }
    }
}

async fn login(client: &CodexClient) -> anyhow::Result<()> {
    let device_code = client.request_device_code().await?;

    println!("Open this URL and sign in:");
    println!("{}", device_code.verification_url);
    println!();
    println!("Enter this code:");
    println!("{}", device_code.user_code);
    println!();
    println!("Waiting for authorization...");

    let tokens = client.complete_device_code_login(&device_code).await?;
    match tokens.chatgpt_account_id() {
        Some(account_id) => println!("Signed in and stored token for account {account_id}."),
        None => println!("Signed in and stored token."),
    }

    Ok(())
}

fn status(client: &CodexClient) -> anyhow::Result<()> {
    match client.token_store().load()? {
        Some(tokens) => match tokens.chatgpt_account_id() {
            Some(account_id) => println!("Token stored for account {account_id}."),
            None => println!("Token stored."),
        },
        None => println!("No token stored."),
    }
    Ok(())
}

fn logout(client: &CodexClient) -> anyhow::Result<()> {
    client.token_store().delete()?;
    println!("Deleted stored token.");
    Ok(())
}

async fn run(
    client: &CodexClient,
    model: String,
    instructions: String,
    input: String,
) -> anyhow::Result<()> {
    let request = CodexTaskRequest::new(model, instructions, input);

    let response = client.create_response_raw(&request).await?;
    print_response_stream(response).await?;
    Ok(())
}

struct ImageCommand {
    model: String,
    instructions: String,
    image: Option<PathBuf>,
    quality: String,
    size: Option<String>,
    action: Option<String>,
    detail: String,
    prompt: String,
}

async fn image_cmd(client: &CodexClient, command: ImageCommand) -> anyhow::Result<()> {
    let ImageCommand {
        model,
        instructions,
        image,
        quality,
        size,
        action,
        detail,
        prompt,
    } = command;

    let action = action.unwrap_or_else(|| {
        if image.is_some() {
            "edit".to_string()
        } else {
            "generate".to_string()
        }
    });

    let mut request = CodexImageGenerationRequest::new(model, prompt)
        .with_instructions(instructions)
        .with_quality(quality)
        .with_action(action);
    if let Some(size) = size {
        request = request.with_size(size);
    }
    if let Some(image) = image {
        request = request
            .with_input_image(CodexInputImage::new(image_data_url(&image)?).with_detail(detail));
    }

    let response = client.create_response_raw(&request).await?;
    let Some(url) = image_response_stream_url(response).await? else {
        anyhow::bail!("No image URL or image result found in response stream");
    };
    println!("{url}");
    Ok(())
}

async fn print_response_stream(response: reqwest::Response) -> anyhow::Result<()> {
    let mut emitted_delta = false;
    let mut final_text = None;
    let mut stream = response.bytes_stream().eventsource();

    while let Some(event) = stream.next().await {
        let event = event?;
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            continue;
        };

        match data.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    print!("{delta}");
                    std::io::stdout().flush()?;
                    emitted_delta = true;
                }
            }
            Some("response.output_text.done") => {
                final_text = data
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
            }
            _ => {}
        }
    }

    if emitted_delta {
        println!();
    } else if let Some(final_text) = final_text {
        println!("{final_text}");
    }

    Ok(())
}

async fn image_response_stream_url(response: reqwest::Response) -> anyhow::Result<Option<String>> {
    let mut stream = response.bytes_stream().eventsource();
    let mut result = None;

    while let Some(event) = stream.next().await {
        let event = event?;
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            continue;
        };
        if let Some(url) = extract_image_url(&data) {
            result = Some(url);
        }
    }

    Ok(result)
}

fn extract_image_url(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if matches!(
                map.get("type").and_then(Value::as_str),
                Some("image_generation_call")
            ) && let Some(url) = extract_image_result(map.get("result")?)
            {
                return Some(url);
            }

            if let Some(call) = map.get("image_generation_call")
                && let Some(url) = extract_image_url(call)
            {
                return Some(url);
            }

            if let Some(url) = map.get("url").and_then(Value::as_str)
                && (url.starts_with("http://")
                    || url.starts_with("https://")
                    || url.starts_with("data:image/"))
            {
                return Some(url.to_string());
            }

            for child in map.values() {
                if let Some(url) = extract_image_url(child) {
                    return Some(url);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(extract_image_url),
        _ => None,
    }
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
            .map(ToOwned::to_owned)
            .or_else(|| map.values().find_map(extract_image_url)),
        Value::Array(items) => items.iter().find_map(extract_image_url),
        _ => None,
    }
}

fn image_data_url(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{};base64,{b64}", image_mime_type(path)))
}

fn image_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "image/png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn infers_image_mime_type_from_extension() {
        assert_eq!(image_mime_type(Path::new("input.jpg")), "image/jpeg");
        assert_eq!(image_mime_type(Path::new("input.webp")), "image/webp");
        assert_eq!(image_mime_type(Path::new("input.png")), "image/png");
    }
}
