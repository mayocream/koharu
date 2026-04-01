use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Document, FontPrediction, TextBlock, TextShaderEffect, TextStrokeStyle, TextStyle};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FontFaceInfo {
    pub family_name: String,
    pub post_script_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MetaInfo {
    pub version: String,
    pub ml_device: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BootstrapConfig {
    pub runtime: BootstrapPathConfig,
    pub models: BootstrapPathConfig,
    pub http: BootstrapHttpConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BootstrapPathConfig {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BootstrapHttpConfig {
    pub proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentSummary {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub revision: u64,
    pub has_segment: bool,
    pub has_inpainted: bool,
    pub has_brush_layer: bool,
    pub has_rendered: bool,
    pub text_block_count: usize,
}

impl From<&Document> for DocumentSummary {
    fn from(document: &Document) -> Self {
        Self {
            id: document.id.clone(),
            name: document.name.clone(),
            width: document.width,
            height: document.height,
            revision: document.revision,
            has_segment: document.segment.is_some(),
            has_inpainted: document.inpainted.is_some(),
            has_brush_layer: document.brush_layer.is_some(),
            has_rendered: document.rendered.is_some(),
            text_block_count: document.text_blocks.len(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProjectStageStatus {
    #[default]
    Idle,
    Ready,
    Stale,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectStageState {
    pub status: ProjectStageStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectPageStages {
    pub detect: ProjectStageState,
    pub ocr: ProjectStageState,
    pub inpaint: ProjectStageState,
    pub translate: ProjectStageState,
    pub render: ProjectStageState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectPageSummary {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub revision: u64,
    pub has_segment: bool,
    pub has_inpainted: bool,
    pub has_brush_layer: bool,
    pub has_rendered: bool,
    pub text_block_count: usize,
    pub stages: ProjectPageStages,
}

impl From<&Document> for ProjectPageSummary {
    fn from(document: &Document) -> Self {
        Self {
            id: document.id.clone(),
            name: document.name.clone(),
            width: document.width,
            height: document.height,
            revision: document.revision,
            has_segment: document.segment.is_some(),
            has_inpainted: document.inpainted.is_some(),
            has_brush_layer: document.brush_layer.is_some(),
            has_rendered: document.rendered.is_some(),
            text_block_count: document.text_blocks.len(),
            stages: ProjectPageStages::default(),
        }
    }
}

impl From<&ProjectPageSummary> for DocumentSummary {
    fn from(page: &ProjectPageSummary) -> Self {
        Self {
            id: page.id.clone(),
            name: page.name.clone(),
            width: page.width,
            height: page.height,
            revision: page.revision,
            has_segment: page.has_segment,
            has_inpainted: page.has_inpainted,
            has_brush_layer: page.has_brush_layer,
            has_rendered: page.has_rendered,
            text_block_count: page.text_block_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub page_count: usize,
    pub updated_at_ms: u64,
    pub current_document_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectManifest {
    pub id: String,
    pub name: String,
    pub page_count: usize,
    pub updated_at_ms: u64,
    pub current_document_id: Option<String>,
    pub pages: Vec<ProjectPageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectPageDetail {
    pub project_id: String,
    pub page: ProjectPageSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TextBlockDetail {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
    pub line_polygons: Option<Vec<[[f32; 2]; 4]>>,
    pub source_direction: Option<crate::TextDirection>,
    pub rendered_direction: Option<crate::TextDirection>,
    pub source_language: Option<String>,
    pub rotation_deg: Option<f32>,
    pub detected_font_size_px: Option<f32>,
    pub detector: Option<String>,
    pub text: Option<String>,
    pub translation: Option<String>,
    pub style: Option<TextStyle>,
    pub font_prediction: Option<FontPrediction>,
}

impl From<&TextBlock> for TextBlockDetail {
    fn from(block: &TextBlock) -> Self {
        Self {
            id: block.id.clone(),
            x: block.x,
            y: block.y,
            width: block.width,
            height: block.height,
            confidence: block.confidence,
            line_polygons: block.line_polygons.clone(),
            source_direction: block.source_direction,
            rendered_direction: block.rendered_direction,
            source_language: block.source_language.clone(),
            rotation_deg: block.rotation_deg,
            detected_font_size_px: block.detected_font_size_px,
            detector: block.detector.clone(),
            text: block.text.clone(),
            translation: block.translation.clone(),
            style: block.style.clone(),
            font_prediction: block.font_prediction.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentDetail {
    pub id: String,
    pub path: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub revision: u64,
    pub text_blocks: Vec<TextBlockDetail>,
}

impl From<&Document> for DocumentDetail {
    fn from(document: &Document) -> Self {
        Self {
            id: document.id.clone(),
            path: document.path.to_string_lossy().to_string(),
            name: document.name.clone(),
            width: document.width,
            height: document.height,
            revision: document.revision,
            text_blocks: document
                .text_blocks
                .iter()
                .map(TextBlockDetail::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TextBlockPatch {
    pub text: Option<String>,
    pub translation: Option<String>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub style: Option<TextStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CreateTextBlock {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ImportMode {
    Replace,
    Append,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportResult {
    pub total_count: usize,
    pub documents: Vec<DocumentSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ExportLayer {
    Rendered,
    Inpainted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportResult {
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LlmModelInfo {
    pub id: String,
    pub languages: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LlmStateStatus {
    Empty,
    Loading,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LlmState {
    pub status: LlmStateStatus,
    pub model_id: Option<String>,
    pub source: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LlmLoadRequest {
    pub id: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub custom_system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LlmPingRequest {
    pub base_url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LlmPingResponse {
    pub ok: bool,
    pub models: Vec<String>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobState {
    pub id: String,
    pub kind: String,
    pub status: JobStatus,
    pub step: Option<String>,
    pub current_document: usize,
    pub total_documents: usize,
    pub current_step_index: usize,
    pub total_steps: usize,
    pub overall_percent: u8,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TransferStatus {
    Started,
    Downloading,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DownloadState {
    pub id: String,
    pub filename: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub status: TransferStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SnapshotEvent {
    pub documents: Vec<DocumentSummary>,
    pub current_project: Option<ProjectSummary>,
    pub current_document_id: Option<String>,
    pub llm: LlmState,
    pub jobs: Vec<JobState>,
    pub downloads: Vec<DownloadState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentsChangedEvent {
    pub documents: Vec<DocumentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentChangedEvent {
    pub document_id: String,
    pub revision: u64,
    pub changed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiKeyValue {
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiKeyResponse {
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RenderRequest {
    pub text_block_id: Option<String>,
    pub shader_effect: Option<TextShaderEffect>,
    pub shader_stroke: Option<TextStrokeStyle>,
    pub font_family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranslateRequest {
    pub text_block_id: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PipelineJobRequest {
    pub document_id: Option<String>,
    pub llm_model_id: Option<String>,
    pub llm_api_key: Option<String>,
    pub llm_base_url: Option<String>,
    pub llm_temperature: Option<f64>,
    pub llm_max_tokens: Option<u32>,
    pub llm_custom_system_prompt: Option<String>,
    pub language: Option<String>,
    pub shader_effect: Option<TextShaderEffect>,
    pub shader_stroke: Option<TextStrokeStyle>,
    pub font_family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MaskRegionRequest {
    pub data: Vec<u8>,
    pub region: Option<Region>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrushRegionRequest {
    pub data: Vec<u8>,
    pub region: Region,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InpaintRegionRequest {
    pub region: Region,
}
