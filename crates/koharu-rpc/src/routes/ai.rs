//! AI workflow routes. These are separate from `/llm/*` because Codex image
//! generation is not a translation model lifecycle concern.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use koharu_app::ai::{CodexAuthStatus, CodexDeviceLogin, CodexImageGenerationOptions};
use koharu_core::{AppEvent, JobFinishedEvent, JobStatus, JobSummary};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::AppState;
use crate::error::{ApiError, ApiResult};
use crate::routes::operations::{register_cancel, unregister_cancel};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(get_codex_auth_status))
        .routes(routes!(start_codex_device_login))
        .routes(routes!(delete_codex_session))
        .routes(routes!(start_codex_image_generation))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodexImageGenerationResponse {
    pub operation_id: String,
}

#[utoipa::path(
    get,
    path = "/ai/codex/auth/status",
    responses((status = 200, body = CodexAuthStatus))
)]
async fn get_codex_auth_status(State(app): State<AppState>) -> ApiResult<Json<CodexAuthStatus>> {
    app.ai
        .codex_auth_status()
        .map(Json)
        .map_err(ApiError::internal)
}

#[utoipa::path(
    post,
    path = "/ai/codex/auth/device-code",
    responses((status = 200, body = CodexDeviceLogin))
)]
async fn start_codex_device_login(
    State(app): State<AppState>,
) -> ApiResult<Json<CodexDeviceLogin>> {
    app.ai
        .start_codex_device_login()
        .await
        .map(Json)
        .map_err(ApiError::internal)
}

#[utoipa::path(delete, path = "/ai/codex/auth/session", responses((status = 204)))]
async fn delete_codex_session(State(app): State<AppState>) -> ApiResult<StatusCode> {
    app.ai.logout_codex().map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/ai/codex/images",
    request_body = CodexImageGenerationOptions,
    responses((status = 200, body = CodexImageGenerationResponse))
)]
async fn start_codex_image_generation(
    State(app): State<AppState>,
    Json(req): Json<CodexImageGenerationOptions>,
) -> ApiResult<Json<CodexImageGenerationResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;

    let operation_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    register_cancel(operation_id.clone(), cancel.clone());

    app.jobs.insert(
        operation_id.clone(),
        JobSummary {
            id: operation_id.clone(),
            kind: "ai".to_string(),
            status: JobStatus::Running,
            error: None,
        },
    );
    app.bus.publish(AppEvent::JobStarted {
        id: operation_id.clone(),
        kind: "ai".to_string(),
    });

    let app_c = app.clone();
    let session_c = session.clone();
    let op_id_c = operation_id.clone();
    tokio::spawn(async move {
        let result = app_c
            .ai
            .generate_codex_page_image(session_c, req, cancel)
            .await;
        let (status, error) = match result {
            Ok(()) => (JobStatus::Completed, None),
            Err(e) if e.to_string().contains("cancelled") => (JobStatus::Cancelled, None),
            Err(e) => {
                tracing::warn!(operation_id = %op_id_c, "Codex image generation failed: {e:#}");
                (JobStatus::Failed, Some(format!("{e:#}")))
            }
        };
        app_c.jobs.insert(
            op_id_c.clone(),
            JobSummary {
                id: op_id_c.clone(),
                kind: "ai".to_string(),
                status,
                error: error.clone(),
            },
        );
        app_c.bus.publish(AppEvent::JobFinished(JobFinishedEvent {
            id: op_id_c.clone(),
            status,
            error,
        }));
        unregister_cancel(&op_id_c);
    });

    Ok(Json(CodexImageGenerationResponse { operation_id }))
}
