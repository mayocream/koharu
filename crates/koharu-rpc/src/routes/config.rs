//! Config routes. Apply via `koharu_app::config::apply_patch`, then persist
//! (config.toml) and broadcast `ConfigChanged`. Provider secrets sync to the
//! keyring via `sync_secrets`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use koharu_app::AppConfig;
use koharu_app::config;
use koharu_core::ConfigPatch;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default()
        .routes(routes!(get_config))
        .routes(routes!(patch_config))
        .routes(routes!(set_provider_secret))
        .routes(routes!(clear_provider_secret))
}

#[utoipa::path(get, path = "/config", responses((status = 200, body = AppConfig)))]
async fn get_config(State(app): State<AppState>) -> ApiResult<Json<AppConfig>> {
    Ok(Json((**app.config.load()).clone()))
}

#[utoipa::path(
    patch,
    path = "/config",
    request_body = ConfigPatch,
    responses((status = 200, body = AppConfig))
)]
async fn patch_config(
    State(app): State<AppState>,
    Json(patch): Json<ConfigPatch>,
) -> ApiResult<Json<AppConfig>> {
    let current = (**app.config.load()).clone();
    let mut next = current;
    config::apply_patch(&mut next, patch);
    config::sync_secrets(&next).map_err(ApiError::internal)?;
    config::save(&next).map_err(ApiError::internal)?;
    app.config.store(Arc::new(next.clone()));
    Ok(Json(next))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSecretRequest {
    pub secret: String,
}

/// Save (or overwrite) the keyring secret for a provider. Creates the
/// provider entry in `config.providers` if it didn't exist. `PUT` because
/// setting the secret is idempotent for the same body.
#[utoipa::path(
    put,
    path = "/config/providers/{id}/secret",
    params(("id" = String, Path, description = "Provider id")),
    request_body = ProviderSecretRequest,
    responses((status = 204))
)]
async fn set_provider_secret(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ProviderSecretRequest>,
) -> ApiResult<StatusCode> {
    let mut next = (**app.config.load()).clone();
    upsert_provider_secret(&mut next, &id, Some(&req.secret));
    config::sync_secrets(&next).map_err(ApiError::internal)?;
    config::save(&next).map_err(ApiError::internal)?;
    app.config.store(Arc::new(next));
    Ok(StatusCode::NO_CONTENT)
}

/// Clear a provider's keyring secret. The provider entry itself is kept.
#[utoipa::path(
    delete,
    path = "/config/providers/{id}/secret",
    params(("id" = String, Path, description = "Provider id")),
    responses((status = 204))
)]
async fn clear_provider_secret(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let mut next = (**app.config.load()).clone();
    upsert_provider_secret(&mut next, &id, None);
    config::sync_secrets(&next).map_err(ApiError::internal)?;
    config::save(&next).map_err(ApiError::internal)?;
    app.config.store(Arc::new(next));
    Ok(StatusCode::NO_CONTENT)
}

fn upsert_provider_secret(config: &mut AppConfig, id: &str, secret: Option<&str>) {
    let redacted = secret.map(config::RedactedSecret::new);
    if let Some(existing) = config.providers.iter_mut().find(|p| p.id == id) {
        existing.api_key = redacted;
    } else {
        config.providers.push(config::ProviderConfig {
            id: id.to_string(),
            base_url: None,
            api_key: redacted,
        });
    }
}
