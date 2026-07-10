//! `GET /meta`, `GET /engines` — server metadata + pipeline engine catalog.

use axum::Json;
use axum::extract::State;
use koharu_core::{EngineCatalog, MetaInfo};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::ApiResult;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(get_meta))
        .routes(routes!(get_engine_catalog))
}

#[utoipa::path(get, path = "/meta", responses((status = 200, body = MetaInfo)))]
async fn get_meta(State(app): State<AppState>) -> ApiResult<Json<MetaInfo>> {
    Ok(Json(MetaInfo {
        version: app.version.to_string(),
        ml_device: device_label(&app),
    }))
}

#[utoipa::path(get, path = "/engines", responses((status = 200, body = EngineCatalog)))]
async fn get_engine_catalog(State(_app): State<AppState>) -> ApiResult<Json<EngineCatalog>> {
    Ok(Json(koharu_app::pipeline::catalog()))
}

fn device_label(_app: &AppState) -> String {
    // TODO: expose current device via koharu-runtime in a follow-up.
    "auto".to_string()
}
