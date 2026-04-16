use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{FontPrediction, TextBlock, TextShaderEffect, TextStrokeStyle, TextStyle};

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, JsonSchema, ToSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct FontFaceInfo {
    pub family_name: String,
    pub post_script_name: String,
    pub source: crate::google_fonts::FontSource,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummary {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub order: u32,
    pub has_segment: bool,
    pub has_inpainted: bool,
    pub has_brush_layer: bool,
    pub has_rendered: bool,
    pub text_block_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
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
    /// Blob hash for the rendered text block sprite.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered: Option<String>,
    /// Actual render area position/size (when bubble expansion is used).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_height: Option<f32>,
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
            rendered: block.rendered.as_ref().map(|r| r.hash().to_string()),
            render_x: block.render_x,
            render_y: block.render_y,
            render_width: block.render_width,
            render_height: block.render_height,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDetail {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub text_blocks: Vec<TextBlockDetail>,
    /// Blob hash for the source image layer.
    pub image: String,
    /// Blob hash for the segmentation mask layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment: Option<String>,
    /// Blob hash for the inpainted layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inpainted: Option<String>,
    /// Blob hash for the brush layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brush_layer: Option<String>,
    /// Blob hash for the rendered composite layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<crate::DocumentStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextBlockInput {
    pub id: Option<String>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: Option<String>,
    pub translation: Option<String>,
    pub style: Option<TextStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextBlockPatch {
    pub text: Option<String>,
    pub translation: Option<String>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub style: Option<TextStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTextBlock {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReorderRequest {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    Replace,
    Append,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub total_count: usize,
    pub documents: Vec<DocumentSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportLayer {
    Rendered,
    Inpainted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub count: usize,
}

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Completed,
    CompletedWithErrors,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    Started,
    Downloading,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadState {
    pub id: String,
    pub filename: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub status: TransferStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEvent {
    pub documents: Vec<DocumentSummary>,
    pub llm: LlmState,
    pub jobs: Vec<JobState>,
    pub downloads: Vec<DownloadState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentsChangedEvent {
    pub documents: Vec<DocumentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentChangedEvent {
    pub document_id: String,
    pub changed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderRequest {
    pub text_block_id: Option<String>,
    pub shader_effect: Option<TextShaderEffect>,
    pub shader_stroke: Option<TextStrokeStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TranslateRequest {
    pub text_block_id: Option<String>,
    pub language: Option<String>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineLlmRequest {
    pub target: LlmTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<LlmGenerationOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineJobRequest {
    pub document_id: Option<String>,
    pub llm: Option<PipelineLlmRequest>,
    pub language: Option<String>,
    pub system_prompt: Option<String>,
    pub shader_effect: Option<TextShaderEffect>,
    pub shader_stroke: Option<TextStrokeStyle>,
    pub default_font: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[cfg(test)]
mod tests {
    use super::JobStatus;

    #[test]
    fn job_status_serializes_completed_with_errors_in_snake_case() {
        let encoded = serde_json::to_string(&JobStatus::CompletedWithErrors).expect("serialize");
        assert_eq!(encoded, "\"completed_with_errors\"");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MaskRegionRequest {
    pub data: Vec<u8>,
    pub region: Option<Region>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrushRegionRequest {
    pub data: Vec<u8>,
    pub region: Region,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InpaintRegionRequest {
    pub region: Region,
}

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
    pub bubble_detectors: Vec<EngineCatalogEntry>,
    pub font_detectors: Vec<EngineCatalogEntry>,
    pub segmenters: Vec<EngineCatalogEntry>,
    pub ocr: Vec<EngineCatalogEntry>,
    pub translators: Vec<EngineCatalogEntry>,
    pub inpainters: Vec<EngineCatalogEntry>,
    pub renderers: Vec<EngineCatalogEntry>,
}
