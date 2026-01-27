use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json, Router,
    body::Body,
    extract::DefaultBodyLimit,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use koharu_macros::routes;
use rust_embed::Embed;
use serde::Serialize;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};

use koharu_ml::DeviceName;

use crate::{app::AppResources, endpoints::*, llm, ml, renderer::Renderer, state::AppState};

#[derive(Clone)]
pub struct ApiState {
    pub resources: AppResources,
}

impl ApiState {
    pub fn app_state(&self) -> &AppState {
        &self.resources.state
    }

    pub fn ml(&self) -> &Arc<ml::Model> {
        &self.resources.ml
    }

    pub fn llm(&self) -> &Arc<llm::Model> {
        &self.resources.llm
    }

    pub fn renderer(&self) -> &Arc<Renderer> {
        &self.resources.renderer
    }

    pub fn ml_device(&self) -> &DeviceName {
        &self.resources.ml_device
    }
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl From<crate::result::CommandError> for ApiError {
    fn from(err: crate::result::CommandError) -> Self {
        Self::bad_request(err.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self::internal(err.to_string())
    }
}

impl From<axum::extract::multipart::MultipartError> for ApiError {
    fn from(err: axum::extract::multipart::MultipartError) -> Self {
        Self::bad_request(err.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Embed)]
#[folder = "$CARGO_WORKSPACE_DIR/ui/out"]
#[allow_missing = true]
struct EmbeddedUi;

async fn serve_embedded(uri: Uri) -> impl IntoResponse {
    let path = uri.path();
    let target = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    embedded_response(target)
        .or_else(|| embedded_response("index.html"))
        .unwrap_or_else(|| (StatusCode::NOT_FOUND, "Not Found").into_response())
}

fn embedded_response(path: &str) -> Option<Response> {
    let asset = EmbeddedUi::get(path)?;
    let mime = asset.metadata.mimetype();
    let mut response = Response::new(Body::from(asset.data.into_owned()));
    if let Ok(ct) = HeaderValue::from_str(mime) {
        response.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    Some(response)
}

fn build_router(state: ApiState) -> Router {
    routes!(
        app_version,
        device,
        open_external,
        get_documents,
        get_document,
        get_thumbnail,
        open_documents,
        save_documents,
        export_document,
        detect,
        ocr,
        inpaint,
        update_inpaint_mask,
        update_brush_layer,
        inpaint_partial,
        render,
        update_text_blocks,
        list_font_families,
        llm_list,
        llm_load,
        llm_offload,
        llm_ready,
        llm_generate,
    )
    .with_state(state)
    .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
    .layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
            .expose_headers(Any)
            .allow_private_network(true),
    )
    .fallback(serve_embedded)
}

pub async fn serve_with_listener(listener: TcpListener, resources: AppResources) -> Result<()> {
    let router = build_router(ApiState { resources });
    tracing::info!("HTTP server listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}

pub async fn serve(bind: String, resources: AppResources) -> Result<()> {
    let listener = TcpListener::bind(&bind).await?;
    serve_with_listener(listener, resources).await
}
