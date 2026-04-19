//! Axum router assembly + OpenAPI descriptor.
//!
//! Each domain registers its routes; this module stitches them into one
//! `OpenApiRouter<ApiState>` + the OpenAPI doc for static export.

use axum::Router;
use axum::extract::DefaultBodyLimit;
use utoipa_axum::router::OpenApiRouter;

use crate::AppState;
use crate::routes;
use crate::{binary, events};

const MAX_BODY_SIZE: usize = 1024 * 1024 * 1024;

/// State threaded through every `State<ApiState>` extractor.
pub type ApiState = AppState;

/// Build the router + OpenAPI doc. Called by the bin and by `router()`.
pub fn api() -> (Router<ApiState>, utoipa::openapi::OpenApi) {
    OpenApiRouter::default()
        .merge(routes::history::router())
        .merge(routes::pages::router())
        .merge(routes::projects::router())
        .merge(routes::config::router())
        .merge(routes::meta::router())
        .merge(routes::fonts::router())
        .merge(routes::llm::router())
        .merge(routes::pipelines::router())
        .merge(routes::downloads::router())
        .merge(routes::operations::router())
        .merge(events::router())
        .merge(binary::router())
        .split_for_parts()
}

/// Ready-to-serve router. `app` becomes shared state. All routes live under
/// `/api/v1` so the UI can reach them through a single proxy prefix.
pub fn router(app: ApiState) -> Router {
    let (inner, _) = api();
    Router::new()
        .nest("/api/v1", inner.with_state(app))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
}
