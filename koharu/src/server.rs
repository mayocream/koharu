use anyhow::Result;
use axum::{
    Router,
    body::Body,
    extract::DefaultBodyLimit,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rust_embed::Embed;
use tokio::net::TcpListener;
use tower_http::cors::{CorsLayer, ExposeHeaders};

use crate::{app::AppResources, endpoints::*};

impl IntoResponse for crate::result::CommandError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response()
    }
}

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

fn build_router(state: AppResources) -> Router {
    Router::new()
        .route("/api/app_version", get(app_version).post(app_version))
        .route("/api/device", get(device).post(device))
        .route("/api/open_external", post(open_external))
        .route("/api/get_documents", get(get_documents).post(get_documents))
        .route("/api/get_document", get(get_document).post(get_document))
        .route("/api/get_thumbnail", get(get_thumbnail).post(get_thumbnail))
        .route("/api/open_documents", post(open_documents))
        .route("/api/save_documents", post(save_documents))
        .route("/api/export_document", post(export_document))
        .route("/api/detect", post(detect))
        .route("/api/ocr", post(ocr))
        .route("/api/inpaint", post(inpaint))
        .route("/api/update_inpaint_mask", post(update_inpaint_mask))
        .route("/api/update_brush_layer", post(update_brush_layer))
        .route("/api/inpaint_partial", post(inpaint_partial))
        .route("/api/render", post(render))
        .route("/api/update_text_blocks", post(update_text_blocks))
        .route(
            "/api/list_font_families",
            get(list_font_families).post(list_font_families),
        )
        .route("/api/llm_list", get(llm_list).post(llm_list))
        .route("/api/llm_load", post(llm_load))
        .route("/api/llm_offload", post(llm_offload))
        .route("/api/llm_ready", get(llm_ready).post(llm_ready))
        .route("/api/llm_generate", post(llm_generate))
        .route("/api/download_progress", get(download_progress))
        .route("/api/process", post(process))
        .route("/api/process_cancel", post(process_cancel))
        .route("/api/process_progress", get(process_progress))
        .with_state(state)
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .layer(
            CorsLayer::very_permissive()
                .expose_headers(ExposeHeaders::list([header::CONTENT_DISPOSITION])),
        )
        .fallback(serve_embedded)
}

pub async fn serve_with_listener(listener: TcpListener, resources: AppResources) -> Result<()> {
    let router = build_router(resources);
    tracing::info!("HTTP server listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}
