use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager, tower::StreamableHttpServerConfig,
};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

use crate::api;
use crate::mcp::KoharuMcp;
use crate::shared::SharedState;
use crate::tracker::Tracker;

/// Resolves a URL path to `(bytes, mime_type)`. Used for serving static UI assets.
pub type AssetResolver = Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;

pub async fn serve_with_listener(
    listener: TcpListener,
    shared: SharedState,
    assets: AssetResolver,
) -> Result<()> {
    let tracker = Tracker::new(&shared);

    let mcp = StreamableHttpService::new(
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

    let router = Router::new()
        .nest("/api/v1", api::router(shared.clone(), tracker))
        .nest_service("/mcp", mcp)
        .layer(CorsLayer::very_permissive())
        .fallback(move |uri: Uri| {
            let assets = assets.clone();
            async move {
                let path = uri.path().trim_start_matches('/');
                let path = if path.is_empty() { "index.html" } else { path };

                serve_file(&assets, path)
                    .or_else(|| serve_file(&assets, "index.html"))
                    .unwrap_or_else(|| (StatusCode::NOT_FOUND, "Not Found").into_response())
            }
        });

    tracing::info!("HTTP server listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}

fn serve_file(assets: &AssetResolver, path: &str) -> Option<Response> {
    let (bytes, mime) = assets(path)?;
    let mut response = Response::new(Body::from(bytes));
    if let Ok(ct) = HeaderValue::from_str(&mime) {
        response.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    Some(response)
}
