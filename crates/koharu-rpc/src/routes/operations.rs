//! Operations registry endpoints.
//!
//! - `GET /operations` — snapshot of every in-flight + recently-completed
//!   pipeline job, including the latest progress tick. Clients poll this
//!   endpoint while running jobs are expected; React Query drives the
//!   cadence on the UI side.
//! - `DELETE /operations/{id}` — unified cancel. Pipeline cancellation
//!   flips the cancel flag registered at start time; download cancellation
//!   is best-effort (HF hub transfers don't expose mid-stream cancel) and
//!   just evicts the row so the UI clears it.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use koharu_core::JobSummary;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::ApiResult;

static CANCELS: OnceLock<DashMap<String, Arc<AtomicBool>>> = OnceLock::new();
fn cancels() -> &'static DashMap<String, Arc<AtomicBool>> {
    CANCELS.get_or_init(DashMap::new)
}

/// Register a cancel flag for an operation id. Called by `pipelines::start_pipeline`.
pub fn register_cancel(id: String, flag: Arc<AtomicBool>) {
    cancels().insert(id, flag);
}

/// Drop a cancel flag once the operation has finished.
pub fn unregister_cancel(id: &str) {
    cancels().remove(id);
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(list_operations))
        .routes(routes!(cancel_operation))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListOperationsResponse {
    pub operations: Vec<JobSummary>,
}

#[utoipa::path(
    get,
    path = "/operations",
    responses((status = 200, body = ListOperationsResponse))
)]
async fn list_operations(State(app): State<AppState>) -> ApiResult<Json<ListOperationsResponse>> {
    let jobs = app.jobs();
    let operations = jobs.iter().map(|e| e.value().clone()).collect();
    Ok(Json(ListOperationsResponse { operations }))
}

#[utoipa::path(
    delete,
    path = "/operations/{id}",
    params(("id" = String, Path, description = "Operation id")),
    responses((status = 204))
)]
async fn cancel_operation(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if let Some(flag) = cancels().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
    // Best-effort download cancel: drop the registry row.
    app.downloads().remove(&id);
    Ok(StatusCode::NO_CONTENT)
}
