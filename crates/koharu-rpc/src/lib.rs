//! HTTP + SSE + MCP transport over `koharu-app::App`.
//!
//! - `api` — assembles the `utoipa_axum::OpenApiRouter` from per-domain modules
//!   under `routes/`.
//! - `routes/*` — one module per domain (scene, projects, config, llm, …).
//!   Each exposes typed handler fns that share a `State<ApiState>`.
//! - `events` — SSE stream (`GET /events`).
//! - `binary` — byte-oriented reads (`GET /scene.bin`, `GET /blobs/:hash`, …).
//! - `mcp` — per-route MCP tools under rmcp.
//! - `server` — bootstrap glue.

pub mod api;
pub mod binary;
pub mod bootstrap;
pub mod error;
pub mod events;
pub mod mcp;
pub mod psd_export;
pub mod routes;
pub mod server;

use std::sync::Arc;

pub use api::{ApiState, api, router};
pub use bootstrap::BootstrapManager;
pub use error::{ApiError, ApiResult};

/// Concrete state threaded through every `State<ApiState>` extractor.
pub type AppState = Arc<BootstrapManager>;
