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
use crate::events::EventHub;
use crate::mcp::KoharuMcp;
use crate::shared::SharedState;

/// An asset returned by the resolver: raw bytes + MIME type.
pub struct Asset {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

/// A function that resolves a path to an asset.
pub type SharedAssetResolver = Arc<dyn Fn(&str) -> Option<Asset> + Send + Sync>;

pub fn asset_resolver<I>(resolvers: I) -> SharedAssetResolver
where
    I: IntoIterator<Item = SharedAssetResolver>,
{
    let resolvers = resolvers.into_iter().collect::<Vec<_>>();
    Arc::new(move |path: &str| resolvers.iter().find_map(|resolver| resolver(path)))
}

fn build_router(shared: SharedState, resolver: SharedAssetResolver) -> Router {
    let events = EventHub::new(shared.clone());
    let cors = CorsLayer::very_permissive();
    let state = api::ApiState {
        resources: shared.clone(),
        events,
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

    let (router, _openapi) = api::openapi_router()
        .nest_service("/mcp", mcp_service)
        .layer(cors)
        .fallback(move |uri: Uri| {
            let resolver = resolver.clone();
            async move { serve_asset(&resolver, uri) }
        })
        .with_state(state)
        .split_for_parts();

    router
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
    shared: SharedState,
    resolver: SharedAssetResolver,
) -> Result<()> {
    let router = build_router(shared, resolver);
    tracing::info!("HTTP server listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}
