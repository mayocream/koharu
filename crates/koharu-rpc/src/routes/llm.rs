//! LLM lifecycle routes. The loaded model is a singleton resource at
//! `/llm/current`: `GET` describes it, `PUT` loads, `DELETE` unloads.
//!
//! - `GET    /llm/current`   — current state (status, target, error)
//! - `PUT    /llm/current`   — load the given target (local or provider)
//! - `DELETE /llm/current`   — unload / release the model
//! - `GET    /llm/catalog`   — available local + provider-backed models

use axum::Json;
use axum::extract::State;
use koharu_core::{LlmCatalog, LlmLoadRequest, LlmState, LlmTargetKind};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(get_current_llm))
        .routes(routes!(put_current_llm))
        .routes(routes!(delete_current_llm))
        .routes(routes!(get_catalog))
}

#[utoipa::path(get, path = "/llm/current", responses((status = 200, body = LlmState)))]
async fn get_current_llm(State(app): State<AppState>) -> ApiResult<Json<LlmState>> {
    Ok(Json(app.llm.snapshot().await))
}

#[utoipa::path(
    put,
    path = "/llm/current",
    request_body = LlmLoadRequest,
    responses((status = 204))
)]
async fn put_current_llm(
    State(app): State<AppState>,
    Json(req): Json<LlmLoadRequest>,
) -> ApiResult<axum::http::StatusCode> {
    let provider_config = match req.target.kind {
        LlmTargetKind::Provider => req.target.provider_id.as_deref().map(|pid| {
            koharu_app::llm::provider_config_from_settings(&app.config.load(), &app.runtime, pid)
        }),
        LlmTargetKind::Local => None,
    };
    // `LlmLoaded` is published from `App::spawn_llm_forwarder` at the real
    // state transition — `load_from_request` may return before the
    // background load task has actually finished.
    app.llm
        .load_from_request(req, provider_config)
        .await
        .map_err(ApiError::internal)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/llm/current", responses((status = 204)))]
async fn delete_current_llm(State(app): State<AppState>) -> ApiResult<axum::http::StatusCode> {
    app.llm.offload().await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/llm/catalog", responses((status = 200, body = LlmCatalog)))]
async fn get_catalog(State(app): State<AppState>) -> ApiResult<Json<LlmCatalog>> {
    let catalog = koharu_app::llm::catalog(&app.config.load(), &app.runtime).await;
    Ok(Json(catalog))
}
