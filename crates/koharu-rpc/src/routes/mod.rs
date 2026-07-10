//! Per-domain route modules. Each exposes `router()` returning an
//! `OpenApiRouter<ApiState>` that can be merged into the top-level router in
//! `api.rs`.

pub mod ai;
pub mod config;
pub mod downloads;
pub mod fonts;
pub mod history;
pub mod llm;
pub mod meta;
pub mod operations;
pub mod pages;
pub mod pipelines;
pub mod projects;
