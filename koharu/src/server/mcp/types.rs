use koharu_core::{CreateTextBlock, LlmLoadRequest, Region, TextShaderEffect, TextStrokeStyle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DocumentIndexParams {
    pub(crate) index: usize,
}

pub(crate) type LoadModelParams = LlmLoadRequest;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TranslateParams {
    pub(crate) index: usize,
    pub(crate) text_block_index: Option<usize>,
    pub(crate) language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ViewImageParams {
    pub(crate) index: usize,
    pub(crate) layer: String,
    pub(crate) max_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ViewTextBlockParams {
    pub(crate) index: usize,
    pub(crate) text_block_index: usize,
    pub(crate) layer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpenDocumentsParams {
    pub(crate) paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExportDocumentParams {
    pub(crate) index: usize,
    pub(crate) output_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RenderParams {
    pub(crate) index: usize,
    pub(crate) text_block_index: Option<usize>,
    pub(crate) shader_effect: Option<TextShaderEffect>,
    pub(crate) shader_stroke: Option<TextStrokeStyle>,
    pub(crate) font_family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProcessParams {
    pub(crate) index: Option<usize>,
    pub(crate) llm_model_id: Option<String>,
    pub(crate) llm_api_key: Option<String>,
    pub(crate) llm_base_url: Option<String>,
    pub(crate) llm_temperature: Option<f64>,
    pub(crate) llm_max_tokens: Option<u32>,
    pub(crate) llm_custom_system_prompt: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) shader_effect: Option<TextShaderEffect>,
    pub(crate) shader_stroke: Option<TextStrokeStyle>,
    pub(crate) font_family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UpdateTextBlockParams {
    pub(crate) index: usize,
    pub(crate) text_block_index: usize,
    pub(crate) translation: Option<String>,
    pub(crate) x: Option<f32>,
    pub(crate) y: Option<f32>,
    pub(crate) width: Option<f32>,
    pub(crate) height: Option<f32>,
    pub(crate) font_families: Option<Vec<String>>,
    pub(crate) font_size: Option<f32>,
    pub(crate) color: Option<String>,
    pub(crate) shader_effect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateTextBlockParams {
    pub(crate) index: usize,
    #[serde(flatten)]
    pub(crate) block: CreateTextBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoveTextBlockParams {
    pub(crate) index: usize,
    pub(crate) text_block_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MaskMorphParams {
    pub(crate) index: usize,
    pub(crate) radius: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InpaintRegionParams {
    pub(crate) index: usize,
    #[serde(flatten)]
    pub(crate) region: Region,
}
