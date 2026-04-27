//! Terminology library routes. Library metadata lives in config.toml; each
//! library's terms live as one CSV file under the configured data path.

use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use koharu_app::config;
use koharu_app::terminology::{
    self, CreateTerminologyLibraryRequest, ImportTerminologyCsvRequest,
    ListTerminologyLibrariesResponse, TerminologyLibrary, TerminologyLibraryPatch,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(list_libraries))
        .routes(routes!(create_library))
        .routes(routes!(update_library))
        .routes(routes!(delete_library))
        .routes(routes!(import_csv))
        .routes(routes!(export_csv))
}

#[utoipa::path(
    get,
    path = "/terminology",
    responses((status = 200, body = ListTerminologyLibrariesResponse))
)]
async fn list_libraries(
    State(app): State<AppState>,
) -> ApiResult<Json<ListTerminologyLibrariesResponse>> {
    let config = (**app.config.load()).clone();
    let libraries = terminology::load_libraries(&config).map_err(ApiError::internal)?;
    Ok(Json(ListTerminologyLibrariesResponse { libraries }))
}

#[utoipa::path(
    post,
    path = "/terminology",
    request_body = CreateTerminologyLibraryRequest,
    responses((status = 200, body = TerminologyLibrary))
)]
async fn create_library(
    State(app): State<AppState>,
    Json(req): Json<CreateTerminologyLibraryRequest>,
) -> ApiResult<Json<TerminologyLibrary>> {
    let mut next = (**app.config.load()).clone();
    let library = terminology::create_library(&mut next, &req.name).map_err(ApiError::internal)?;
    persist_config(&app, next)?;
    Ok(Json(library))
}

#[utoipa::path(
    patch,
    path = "/terminology/{id}",
    params(("id" = String, Path, description = "Terminology library id")),
    request_body = TerminologyLibraryPatch,
    responses((status = 200, body = TerminologyLibrary))
)]
async fn update_library(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<TerminologyLibraryPatch>,
) -> ApiResult<Json<TerminologyLibrary>> {
    let mut next = (**app.config.load()).clone();
    let library = terminology::update_library(&mut next, &id, patch)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("terminology library {id}")))?;
    persist_config(&app, next)?;
    Ok(Json(library))
}

#[utoipa::path(
    delete,
    path = "/terminology/{id}",
    params(("id" = String, Path, description = "Terminology library id")),
    responses((status = 204))
)]
async fn delete_library(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let mut next = (**app.config.load()).clone();
    let deleted = terminology::delete_library(&mut next, &id).map_err(ApiError::internal)?;
    if !deleted {
        return Err(ApiError::not_found(format!("terminology library {id}")));
    }
    persist_config(&app, next)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/terminology/{id}/import",
    params(("id" = String, Path, description = "Terminology library id")),
    request_body = ImportTerminologyCsvRequest,
    responses((status = 200, body = TerminologyLibrary))
)]
async fn import_csv(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ImportTerminologyCsvRequest>,
) -> ApiResult<Json<TerminologyLibrary>> {
    let mut next = (**app.config.load()).clone();
    let library = terminology::import_csv(&mut next, &id, &req.csv)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("terminology library {id}")))?;
    persist_config(&app, next)?;
    Ok(Json(library))
}

#[utoipa::path(
    get,
    path = "/terminology/{id}/export",
    params(("id" = String, Path, description = "Terminology library id")),
    responses((status = 200, content_type = "text/csv"))
)]
async fn export_csv(State(app): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let config = (**app.config.load()).clone();
    let csv = terminology::export_csv(&config, &id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("terminology library {id}")))?;
    let name = config
        .terminology_libraries
        .iter()
        .find(|library| library.id == id)
        .map(|library| sanitize_filename(&library.name))
        .unwrap_or_else(|| "terminology".to_string());
    Ok(csv_response(csv, &format!("{name}.csv")))
}

fn persist_config(app: &AppState, next: koharu_app::AppConfig) -> ApiResult<()> {
    config::sync_secrets(&next).map_err(ApiError::internal)?;
    config::save(&next).map_err(ApiError::internal)?;
    app.config.store(Arc::new(next));
    Ok(())
}

fn csv_response(csv: String, filename: &str) -> Response {
    let mut response = Response::new(Body::from(csv));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response.into_response()
}

fn sanitize_filename(name: &str) -> String {
    let out = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect::<String>();
    if out.is_empty() {
        "terminology".to_string()
    } else {
        out
    }
}
