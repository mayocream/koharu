use anyhow::{Context as _, Result, bail};
use reqwest::{Client, StatusCode};
use serde_json::Value;

use crate::Reasoning;

use super::{CodexModel, auth::Auth};

const MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models?client_version=0.146.0";
const MAX_CATALOG_BYTES: usize = 1024 * 1024;

pub(super) async fn models(client: &Client, auth: &Auth) -> Result<Vec<CodexModel>> {
    let mut session = auth.session().await?;
    let mut response = request(client, &session).await?;
    if response.status() == StatusCode::UNAUTHORIZED {
        session = auth.force_refresh().await?;
        response = request(client, &session).await?;
    }
    if !response.status().is_success() {
        let status = response.status();
        let mut body = response.text().await.unwrap_or_default();
        body.truncate(16 * 1024);
        bail!("Codex model discovery returned {status}: {body}");
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_CATALOG_BYTES {
        bail!("Codex model catalog exceeded {MAX_CATALOG_BYTES} bytes");
    }
    let value: Value = serde_json::from_slice(&bytes).context("invalid Codex model catalog")?;
    let rows = value
        .get("models")
        .and_then(Value::as_array)
        .context("Codex model catalog must contain a models array")?;
    let models = rows.iter().filter_map(parse).collect::<Vec<_>>();
    if models.is_empty() {
        bail!("the signed-in Codex account has no available models");
    }
    Ok(models)
}

async fn request(client: &Client, session: &super::auth::Session) -> Result<reqwest::Response> {
    Ok(client
        .get(MODELS_URL)
        .bearer_auth(&session.access)
        .header("chatgpt-account-id", &session.account.id)
        .header("originator", "koharu")
        .header("accept", "application/json")
        .send()
        .await?)
}

fn parse(value: &Value) -> Option<CodexModel> {
    let visibility = value
        .get("visibility")
        .and_then(Value::as_str)
        .unwrap_or("list");
    if !visibility.eq_ignore_ascii_case("list")
        || value
            .get("show_in_picker")
            .or_else(|| value.get("showInPicker"))
            .and_then(Value::as_bool)
            == Some(false)
    {
        return None;
    }
    let id = value
        .get("slug")
        .or_else(|| value.get("id"))?
        .as_str()?
        .trim();
    if id.is_empty() {
        return None;
    }
    let reasoning = value
        .get("supported_reasoning_levels")
        .or_else(|| value.get("supportedReasoningLevels"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|level| {
            level
                .as_str()
                .or_else(|| level.get("effort")?.as_str())
                .and_then(|level| match level {
                    "low" => Some(Reasoning::Low),
                    "medium" => Some(Reasoning::Medium),
                    "high" => Some(Reasoning::High),
                    "xhigh" => Some(Reasoning::Xhigh),
                    "max" => Some(Reasoning::Max),
                    "ultra" => Some(Reasoning::Ultra),
                    _ => None,
                })
        })
        .fold(Vec::new(), |mut levels, level| {
            if !levels.contains(&level) {
                levels.push(level);
            }
            levels
        });
    Some(CodexModel {
        id: id.to_owned(),
        name: value
            .get("display_name")
            .or_else(|| value.get("displayName"))
            .and_then(Value::as_str)
            .unwrap_or(id)
            .to_owned(),
        reasoning,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reasoning_aliases_are_normalized_once() {
        let model = parse(&json!({
            "slug": "codex",
            "supported_reasoning_levels": ["low", "medium", "high", "xhigh", "max", "ultra"]
        }))
        .unwrap();

        assert_eq!(
            model.reasoning,
            [
                Reasoning::Low,
                Reasoning::Medium,
                Reasoning::High,
                Reasoning::Xhigh,
                Reasoning::Max,
                Reasoning::Ultra,
            ]
        );
    }
}
