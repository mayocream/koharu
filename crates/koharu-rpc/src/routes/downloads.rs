//! Downloads registry endpoints.
//!
//! - `POST /downloads` — start a model-package download (non-blocking).
//! - `GET /downloads`  — snapshot of every in-flight + recently-finished
//!   package download. Clients poll while any are in flight.
//!
//! HF hub downloads aren't cleanly cancellable mid-stream; cancellation via
//! `DELETE /operations/{id}` evicts the registry row and the transfer
//! completes silently.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use koharu_core::DownloadProgress;
use koharu_runtime::packages::PackageCatalog;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(start_download))
        .routes(routes!(list_downloads))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDownloadsResponse {
    pub downloads: Vec<DownloadProgress>,
}

#[utoipa::path(
    get,
    path = "/downloads",
    responses((status = 200, body = ListDownloadsResponse))
)]
async fn list_downloads(State(app): State<AppState>) -> ApiResult<Json<ListDownloadsResponse>> {
    let downloads_state = app.downloads();
    let downloads = downloads_state.iter().map(|e| e.value().clone()).collect();
    Ok(Json(ListDownloadsResponse { downloads }))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartDownloadRequest {
    /// Package id, as declared via `declare_hf_model_package!`
    /// (e.g. `"model:comic-text-detector:yolo-v5"`).
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartDownloadResponse {
    /// Operation id. Reusing the package id keeps ids meaningful for clients
    /// watching progress events.
    pub operation_id: String,
}

#[utoipa::path(
    post,
    path = "/downloads",
    request_body = StartDownloadRequest,
    responses((status = 202, body = StartDownloadResponse))
)]
async fn start_download(
    State(app): State<AppState>,
    Json(req): Json<StartDownloadRequest>,
) -> ApiResult<(StatusCode, Json<StartDownloadResponse>)> {
    let catalog = PackageCatalog::discover();
    let pkg = catalog
        .all()
        .find(|p| p.id == req.model_id)
        .ok_or_else(|| ApiError::not_found(format!("unknown package {}", req.model_id)))?;
    let runtime = app.runtime();
    tokio::spawn(async move {
        if let Err(e) = (pkg.ensure)(&runtime).await {
            tracing::error!(package = pkg.id, "download failed: {e:#}");
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(StartDownloadResponse {
            operation_id: req.model_id,
        }),
    ))
}
