use koharu_core::{CreateTextBlock, Region, TextShaderEffect, TextStrokeStyle};

#[derive(Clone, Debug)]
pub(crate) struct RenderJob {
    pub(crate) document_index: usize,
    pub(crate) text_block_index: Option<usize>,
    pub(crate) shader_effect: Option<TextShaderEffect>,
    pub(crate) shader_stroke: Option<TextStrokeStyle>,
    pub(crate) font_family: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ModelCatalogQuery {
    pub(crate) language: Option<String>,
    pub(crate) openai_compatible_base_url: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ApiKeyUpdate {
    pub(crate) provider: String,
    pub(crate) api_key: String,
}

#[derive(Clone, Debug)]
pub(crate) struct LlmLoadJob {
    pub(crate) id: String,
    pub(crate) api_key: Option<String>,
    pub(crate) base_url: Option<String>,
    pub(crate) temperature: Option<f64>,
    pub(crate) max_tokens: Option<u32>,
    pub(crate) custom_system_prompt: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct TranslateJob {
    pub(crate) document_index: usize,
    pub(crate) text_block_index: Option<usize>,
    pub(crate) language: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct PipelineJob {
    pub(crate) document_index: Option<usize>,
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

#[derive(Clone, Debug)]
pub(crate) struct TextBlockUpdate {
    pub(crate) document_index: usize,
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

#[derive(Clone, Debug)]
pub(crate) struct CreateTextBlockJob {
    pub(crate) document_index: usize,
    pub(crate) block: CreateTextBlock,
}

#[derive(Clone, Debug)]
pub(crate) struct RemoveTextBlockJob {
    pub(crate) document_index: usize,
    pub(crate) text_block_index: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct MaskMorphJob {
    pub(crate) document_index: usize,
    pub(crate) radius: u8,
}

#[derive(Clone, Debug)]
pub(crate) struct ImageRegion {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl From<Region> for ImageRegion {
    fn from(value: Region) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

impl From<&Region> for ImageRegion {
    fn from(value: &Region) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct InpaintMaskUpdate {
    pub(crate) document_index: usize,
    pub(crate) mask: Vec<u8>,
    pub(crate) region: Option<ImageRegion>,
}

#[derive(Clone, Debug)]
pub(crate) struct BrushLayerUpdate {
    pub(crate) document_index: usize,
    pub(crate) patch: Vec<u8>,
    pub(crate) region: ImageRegion,
}

#[derive(Clone, Debug)]
pub(crate) struct PartialInpaintJob {
    pub(crate) document_index: usize,
    pub(crate) region: ImageRegion,
}
