use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use dashmap::DashMap;
use image::DynamicImage;
use koharu_ai::codex::{CodexClient, CodexConfig};
use koharu_ai::{AiImageProvider, AiImageRequest};
use koharu_core::{
    BlobRef, ImageData, ImageDataPatch, ImageRole, Node, NodeDataPatch, NodeId, NodeKind,
    NodePatch, Op, PageId, Scene, Transform,
};
use koharu_runtime::{RuntimeHttpClient, RuntimeManager};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::Instrument as _;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::session::ProjectSession;

const DEFAULT_CODEX_IMAGE_MODEL: &str = "gpt-5.5";
const DEFAULT_CODEX_IMAGE_INSTRUCTIONS: &str = "Generate or edit the requested image.";
const DEFAULT_CODEX_IMAGE_QUALITY: &str = "high";

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodexAuthAttemptStatus {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceLogin {
    pub login_id: String,
    pub verification_url: String,
    pub user_code: String,
    pub interval_seconds: u64,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceLoginStatus {
    pub login_id: String,
    pub status: CodexAuthAttemptStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodexAuthStatus {
    pub signed_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<CodexDeviceLoginStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodexImageGenerationOptions {
    pub page_id: PageId,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
}

#[derive(Debug, Clone)]
struct LoginAttempt {
    status: CodexAuthAttemptStatus,
    account_id: Option<String>,
    error: Option<String>,
}

pub struct AiManager {
    codex: CodexClient,
    http_client: RuntimeHttpClient,
    codex_device_timeout: Duration,
    codex_logins: Arc<DashMap<String, LoginAttempt>>,
    latest_codex_login: RwLock<Option<String>>,
}

impl AiManager {
    pub fn new(runtime: &RuntimeManager) -> Self {
        let config = CodexConfig::default();
        let codex_device_timeout = config.device_auth_timeout;
        Self {
            codex: CodexClient::with_http_client(config, runtime.http_client()),
            http_client: runtime.http_client(),
            codex_device_timeout,
            codex_logins: Arc::new(DashMap::new()),
            latest_codex_login: RwLock::new(None),
        }
    }

    pub fn codex_auth_status(&self) -> Result<CodexAuthStatus> {
        let tokens = self.codex.token_store().load()?;
        let account_id = tokens
            .as_ref()
            .and_then(|tokens| tokens.chatgpt_account_id());
        let login = self
            .latest_codex_login
            .read()
            .as_ref()
            .and_then(|id| {
                self.codex_logins
                    .get(id)
                    .map(|entry| (id.clone(), entry.clone()))
            })
            .map(|(login_id, attempt)| CodexDeviceLoginStatus {
                login_id,
                status: attempt.status,
                account_id: attempt.account_id,
                error: attempt.error,
            });

        Ok(CodexAuthStatus {
            signed_in: tokens.is_some(),
            account_id,
            login,
        })
    }

    pub async fn start_codex_device_login(self: &Arc<Self>) -> Result<CodexDeviceLogin> {
        let device_code = self.codex.request_device_code().await?;
        let login_id = Uuid::new_v4().to_string();
        self.codex_logins.insert(
            login_id.clone(),
            LoginAttempt {
                status: CodexAuthAttemptStatus::Pending,
                account_id: None,
                error: None,
            },
        );
        *self.latest_codex_login.write() = Some(login_id.clone());

        let manager = Arc::clone(self);
        let device_code_for_task = device_code.clone();
        let login_id_for_task = login_id.clone();
        tokio::spawn(async move {
            let result = manager
                .codex
                .complete_device_code_login(&device_code_for_task)
                .await;
            let attempt = match result {
                Ok(tokens) => LoginAttempt {
                    status: CodexAuthAttemptStatus::Succeeded,
                    account_id: tokens.chatgpt_account_id(),
                    error: None,
                },
                Err(err) => LoginAttempt {
                    status: CodexAuthAttemptStatus::Failed,
                    account_id: None,
                    error: Some(format!("{err:#}")),
                },
            };
            manager.codex_logins.insert(login_id_for_task, attempt);
        });

        let interval_seconds = device_code.interval().as_secs().max(1);
        Ok(CodexDeviceLogin {
            login_id,
            verification_url: device_code.verification_url,
            user_code: device_code.user_code,
            interval_seconds,
            timeout_seconds: self.codex_device_timeout.as_secs(),
        })
    }

    pub fn logout_codex(&self) -> Result<()> {
        self.codex.token_store().delete()?;
        Ok(())
    }

    pub async fn generate_codex_page_image(
        &self,
        session: Arc<ProjectSession>,
        options: CodexImageGenerationOptions,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<()> {
        let workflow_span = tracing::info_span!(
            "codex_image_generation_workflow",
            page_id = %options.page_id
        );
        async move {
            let prompt = options.prompt.trim().to_string();
            if prompt.is_empty() {
                bail!("prompt is required");
            }

            let source = tracing::info_span!("codex_source_image_load").in_scope(|| {
                let scene = session.scene_snapshot();
                let (_, image_data) = source_image(&scene, options.page_id)?;
                session.blobs.load_image(&image_data.blob)
            })?;

            let source_data_url = tracing::info_span!("codex_source_image_encode")
                .in_scope(|| image_data_url(&source))?;
            tracing::info!(bytes = source_data_url.len(), "encoded Codex source image");

            check_cancelled(&cancel)?;
            let mut request = AiImageRequest::new(
                options
                    .model
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_CODEX_IMAGE_MODEL.to_string()),
                prompt,
            )
            .with_input_image(source_data_url);
            request.instructions = options
                .instructions
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_CODEX_IMAGE_INSTRUCTIONS.to_string());
            request.quality = options
                .quality
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_CODEX_IMAGE_QUALITY.to_string());
            request.size = options
                .size
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some("auto".to_string()));
            request.action = Some("edit".to_string());

            let result = self
                .codex
                .generate_image(request)
                .instrument(tracing::info_span!("codex_image_request"))
                .await?;
            tracing::info!("Codex image request completed");

            check_cancelled(&cancel)?;
            let generated_bytes = self
                .load_generated_image_bytes(&result.image_url)
                .instrument(tracing::info_span!("codex_generated_image_load"))
                .await?;
            tracing::info!(
                bytes = generated_bytes.len(),
                "loaded Codex generated image bytes"
            );

            let (width, height, blob) = tracing::info_span!("codex_generated_image_store")
                .in_scope(|| {
                    let generated = image::load_from_memory(&generated_bytes)
                        .with_context(|| "failed to decode Codex image result")?;
                    let (width, height) = image_dimensions(&generated);
                    let blob = session.blobs.put_webp(&generated)?;
                    Ok::<_, anyhow::Error>((width, height, blob))
                })?;
            tracing::info!(width, height, "decoded and stored Codex generated image");

            check_cancelled(&cancel)?;
            let scene = session.scene_snapshot();
            let op = upsert_image_blob(
                &scene,
                options.page_id,
                ImageRole::Rendered,
                blob,
                width,
                height,
            )?;
            session.apply(Op::Batch {
                ops: vec![op],
                label: format!("codex-image: page {}", options.page_id),
            })?;
            tracing::info!("finished Codex image generation workflow");
            Ok(())
        }
        .instrument(workflow_span)
        .await
    }

    async fn load_generated_image_bytes(&self, url: &str) -> Result<Vec<u8>> {
        if let Some(bytes) = decode_data_image_url(url)? {
            return Ok(bytes);
        }

        if !(url.starts_with("http://") || url.starts_with("https://")) {
            bail!("unsupported Codex image result URL: {url}");
        }

        let response = self.http_client.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("failed to fetch Codex image result ({status}): {body}");
        }
        Ok(response.bytes().await?.to_vec())
    }
}

fn check_cancelled(cancel: &std::sync::atomic::AtomicBool) -> Result<()> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        bail!("cancelled");
    }
    Ok(())
}

fn source_image(scene: &Scene, page_id: PageId) -> Result<(NodeId, &ImageData)> {
    let page = scene
        .page(page_id)
        .with_context(|| format!("page {} not found", page_id))?;
    page.nodes
        .iter()
        .find_map(|(id, node)| match &node.kind {
            NodeKind::Image(image) if image.role == ImageRole::Source => Some((*id, image)),
            _ => None,
        })
        .ok_or_else(|| anyhow!("page has no Source image node"))
}

fn image_data_url(image: &DynamicImage) -> Result<String> {
    let mut buf = Cursor::new(Vec::new());
    image.write_to(&mut buf, image::ImageFormat::Png)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
    Ok(format!("data:image/png;base64,{encoded}"))
}

fn decode_data_image_url(url: &str) -> Result<Option<Vec<u8>>> {
    let Some(rest) = url.strip_prefix("data:image/") else {
        return Ok(None);
    };
    let Some((_, data)) = rest.split_once(',') else {
        bail!("invalid data image URL");
    };
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .context("failed to decode data image URL")?;
    Ok(Some(decoded))
}

fn image_dimensions(image: &DynamicImage) -> (u32, u32) {
    use image::GenericImageView as _;
    image.dimensions()
}

fn upsert_image_blob(
    scene: &Scene,
    page: PageId,
    role: ImageRole,
    blob: BlobRef,
    natural_width: u32,
    natural_height: u32,
) -> Result<Op> {
    let page_ref = scene
        .page(page)
        .with_context(|| format!("page {} not found", page))?;

    if let Some((node_id, _)) = page_ref
        .nodes
        .iter()
        .find_map(|(id, node)| match &node.kind {
            NodeKind::Image(image) if image.role == role => Some((*id, image)),
            _ => None,
        })
    {
        return Ok(Op::UpdateNode {
            page,
            id: node_id,
            patch: NodePatch {
                data: Some(NodeDataPatch::Image(ImageDataPatch {
                    blob: Some(blob),
                    opacity: None,
                    name: None,
                    natural_width: Some(natural_width),
                    natural_height: Some(natural_height),
                })),
                transform: None,
                visible: None,
            },
            prev: NodePatch::default(),
        });
    }

    let at = if role == ImageRole::Inpainted {
        1.min(page_ref.nodes.len())
    } else {
        page_ref.nodes.len()
    };
    Ok(Op::AddNode {
        page,
        node: Node {
            id: NodeId::new(),
            transform: Transform::default(),
            visible: role != ImageRole::Rendered,
            kind: NodeKind::Image(ImageData {
                role,
                blob,
                opacity: 1.0,
                natural_width,
                natural_height,
                name: None,
            }),
        },
        at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_data_image_url() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(b"png");
        let decoded = decode_data_image_url(&format!("data:image/png;base64,{encoded}")).unwrap();
        assert_eq!(decoded, Some(b"png".to_vec()));
    }

    #[test]
    fn ignores_non_data_url() {
        assert!(
            decode_data_image_url("https://example.test/image.png")
                .unwrap()
                .is_none()
        );
    }
}
