//! Server bootstrap — attaches the router to an axum listener.
//!
//! Also exposes an `AssetResolver` hook so the Tauri binary can bolt its
//! embedded frontend onto unmatched routes.

use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header::CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

use crate::AppState;
use crate::api;

/// Function that maps a URL path (e.g. `"/index.html"`) to `(bytes, mime)`.
/// Returning `None` signals a 404 fall-through.
pub type AssetResolver = Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;

/// Wrap `router(app)` with CORS + mount MCP at `/mcp`.
pub fn router_for(app: AppState) -> Router {
    let base = api::router(app.clone()).layer(CorsLayer::very_permissive());
    crate::mcp::mount(base, app)
}

/// Same as `router_for` but installs `resolver` as a fallback, serving
/// embedded frontend assets for unmatched GET requests.
pub fn router_with_assets(app: AppState, resolver: AssetResolver) -> Router {
    router_for(app).fallback(move |req: Request<Body>| {
        let resolver = resolver.clone();
        async move { serve_asset(resolver, req).await }
    })
}

async fn serve_asset(resolver: AssetResolver, req: Request<Body>) -> Response {
    if req.method() != axum::http::Method::GET {
        return (StatusCode::METHOD_NOT_ALLOWED, "method not allowed").into_response();
    }
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some((bytes, mime)) = resolver(path)
        && let Ok(header) = HeaderValue::from_str(&mime)
    {
        let mut resp = Response::new(Body::from(bytes));
        resp.headers_mut().insert(CONTENT_TYPE, header);
        return resp;
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

/// Serve HTTP on an already-bound listener. Tauri-friendly.
pub async fn serve_with_listener(listener: TcpListener, app: AppState) -> Result<()> {
    axum::serve(listener, router_for(app)).await?;
    Ok(())
}

/// Variant that installs embedded assets as the fallback. Used by the Tauri
/// production build to serve the bundled UI.
pub async fn serve_with_listener_and_assets(
    listener: TcpListener,
    app: AppState,
    resolver: AssetResolver,
) -> Result<()> {
    axum::serve(listener, router_with_assets(app, resolver)).await?;
    Ok(())
}
