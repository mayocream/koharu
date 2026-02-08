use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use tauri::AssetResolver;
use tokio::net::TcpListener;

use crate::rpc::{self, SharedResources, WsState};

fn build_router(shared: SharedResources, resolver: Arc<AssetResolver<tauri::Wry>>) -> Router {
    let ws_state = WsState { resources: shared };

    Router::new()
        .route("/ws", get(rpc::ws_handler))
        .with_state(ws_state)
        .fallback(move |uri: Uri| {
            let resolver = resolver.clone();
            async move { serve_asset(&resolver, uri) }
        })
}

fn serve_asset(resolver: &AssetResolver<tauri::Wry>, uri: Uri) -> Response {
    let path = uri.path();
    let target = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    resolve_asset(resolver, target)
        .or_else(|| resolve_asset(resolver, "index.html"))
        .unwrap_or_else(|| (StatusCode::NOT_FOUND, "Not Found").into_response())
}

fn resolve_asset(resolver: &AssetResolver<tauri::Wry>, path: &str) -> Option<Response> {
    let asset = resolver.get(path.to_string())?;
    let mut response = Response::new(Body::from(asset.bytes));
    if let Ok(ct) = HeaderValue::from_str(&asset.mime_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    Some(response)
}

pub async fn serve_with_listener(
    listener: TcpListener,
    shared: SharedResources,
    resolver: Arc<AssetResolver<tauri::Wry>>,
) -> Result<()> {
    let router = build_router(shared, resolver);
    tracing::info!("HTTP server listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}
