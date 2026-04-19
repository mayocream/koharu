//! Non-scene protocol types: metadata, LLM lifecycle, engine catalog, config.
//!
//! Scene ops live in `op.rs`; push events in `events.rs`. Per-route request
//! DTOs (multipart import, pipeline start) live in `koharu-rpc/src/routes/`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::google_fonts::FontSource;

// ---------------------------------------------------------------------------
// Meta / fonts
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, JsonSchema, ToSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct FontFaceInfo {
    pub family_name: String,
    pub post_script_name: String,
    pub source: FontSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetaInfo {
    pub version: String,
    pub ml_device: String,
}

// ---------------------------------------------------------------------------
// Region (generic pixel rectangle)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// LLM lifecycle
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LlmStateStatus {
    Empty,
    Loading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmState {
    pub status: LlmStateStatus,
    pub target: Option<LlmTarget>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmGenerationOptions {
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub custom_system_prompt: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmTargetKind {
    Local,
    Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LlmTarget {
    pub kind: LlmTargetKind,
    pub model_id: String,
    pub provider_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmLoadRequest {
    pub target: LlmTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<LlmGenerationOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmCatalogModel {
    pub target: LlmTarget,
    pub name: String,
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderCatalogStatus {
    Ready,
    MissingConfiguration,
    DiscoveryFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderCatalog {
    pub id: String,
    pub name: String,
    pub requires_api_key: bool,
    pub requires_base_url: bool,
    pub has_api_key: bool,
    pub base_url: Option<String>,
    pub status: LlmProviderCatalogStatus,
    pub error: Option<String>,
    pub models: Vec<LlmCatalogModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmCatalog {
    pub local_models: Vec<LlmCatalogModel>,
    pub providers: Vec<LlmProviderCatalog>,
}

// ---------------------------------------------------------------------------
// Pipeline request shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineLlmRequest {
    pub target: LlmTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<LlmGenerationOptions>,
}

// ---------------------------------------------------------------------------
// Engine catalog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EngineCatalogEntry {
    pub id: String,
    pub name: String,
    pub produces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EngineCatalog {
    pub detectors: Vec<EngineCatalogEntry>,
    pub font_detectors: Vec<EngineCatalogEntry>,
    pub segmenters: Vec<EngineCatalogEntry>,
    pub bubble_segmenters: Vec<EngineCatalogEntry>,
    pub ocr: Vec<EngineCatalogEntry>,
    pub translators: Vec<EngineCatalogEntry>,
    pub inpainters: Vec<EngineCatalogEntry>,
    pub renderers: Vec<EngineCatalogEntry>,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Sparse patch for `koharu_app::AppConfig`. Missing fields mean "leave
/// as-is". The `providers` field, if present, replaces the whole provider
/// list — we do not merge by id because ordering is meaningful.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPatch {
    #[serde(default)]
    pub data: Option<DataConfigPatch>,
    #[serde(default)]
    pub http: Option<HttpConfigPatch>,
    #[serde(default)]
    pub pipeline: Option<PipelineConfigPatch>,
    /// If present, replaces the entire list. Api_key values of `"[REDACTED]"`
    /// are interpreted as "leave the existing secret alone".
    #[serde(default)]
    pub providers: Option<Vec<ProviderPatch>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataConfigPatch {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpConfigPatch {
    pub connect_timeout: Option<u64>,
    pub read_timeout: Option<u64>,
    pub max_retries: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineConfigPatch {
    pub detector: Option<String>,
    pub font_detector: Option<String>,
    pub segmenter: Option<String>,
    pub bubble_segmenter: Option<String>,
    pub ocr: Option<String>,
    pub translator: Option<String>,
    pub inpainter: Option<String>,
    pub renderer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPatch {
    pub id: String,
    pub base_url: Option<String>,
    /// `"[REDACTED]"` → keep existing keyring secret; empty → clear; otherwise save.
    pub api_key: Option<String>,
}
