use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager, tower::StreamableHttpServerConfig,
};
use tokio::net::TcpListener;

use crate::mcp::KoharuMcp;
use crate::rpc::{self, WsState};
use crate::shared::SharedResources;

/// An asset returned by the resolver: raw bytes + MIME type.
pub struct Asset {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

/// A function that resolves a path to an asset.
pub type SharedAssetResolver = Arc<dyn Fn(&str) -> Option<Asset> + Send + Sync>;

fn build_router(shared: SharedResources, resolver: SharedAssetResolver) -> Router {
    let ws_state = WsState {
        resources: shared.clone(),
    };

    let mcp_service = StreamableHttpService::new(
        {
            let shared = shared.clone();
            move || Ok(KoharuMcp::new(shared.clone()))
        },
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            sse_retry: None,
            ..Default::default()
        },
    );

    Router::new()
        .route("/ws", get(rpc::ws_handler))
        .with_state(ws_state)
        .nest_service("/mcp", mcp_service)
        .fallback(move |uri: Uri| {
            let resolver = resolver.clone();
            async move { serve_asset(&resolver, uri) }
        })
}

fn serve_asset(resolver: &SharedAssetResolver, uri: Uri) -> Response {
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

fn resolve_asset(resolver: &SharedAssetResolver, path: &str) -> Option<Response> {
    let asset = resolver(path)?;
    let mut response = Response::new(Body::from(asset.bytes));
    if let Ok(ct) = HeaderValue::from_str(&asset.mime_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    Some(response)
}

pub async fn serve_with_listener(
    listener: TcpListener,
    shared: SharedResources,
    resolver: SharedAssetResolver,
) -> Result<()> {
    let router = build_router(shared, resolver);
    tracing::info!("HTTP server listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}
