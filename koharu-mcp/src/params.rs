use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
pub(crate) struct IndexParam {
    /// Document index (0-based)
    pub index: usize,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ViewImageParams {
    /// Document index (0-based)
    pub index: usize,
    /// Which image layer to view: "original", "segment", "inpainted", or "rendered"
    pub layer: String,
    /// Maximum dimension (longest edge) for the returned image. Default 1024
    pub max_size: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ViewTextBlockParams {
    /// Document index (0-based)
    pub index: usize,
    /// Text block index (0-based)
    pub text_block_index: usize,
    /// Which layer to crop from: "original" or "rendered". Default "original"
    pub layer: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct OpenDocumentsParams {
    /// File paths to open
    pub paths: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ExportDocumentParams {
    /// Document index (0-based)
    pub index: usize,
    /// Output file path to save the rendered image
    pub output_path: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct RenderParams {
    /// Document index (0-based)
    pub index: usize,
    /// Optional text block index to render (omit for all blocks)
    pub text_block_index: Option<usize>,
    /// Shader effect: "normal", "antique", "metal", "manga", "motionblur"
    pub shader_effect: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct LlmLoadParams {
    /// Model ID (e.g. "sakura-galtransl-7b-v3.7")
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct LlmGenerateParams {
    /// Document index (0-based)
    pub index: usize,
    /// Optional text block index (omit for all blocks)
    pub text_block_index: Option<usize>,
    /// Target language override
    pub language: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ProcessParams {
    /// Document index (omit for all documents)
    pub index: Option<usize>,
    /// LLM model ID to load
    pub llm_model_id: Option<String>,
    /// Target language
    pub language: Option<String>,
    /// Shader effect
    pub shader_effect: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct UpdateTextBlockParams {
    /// Document index (0-based)
    pub index: usize,
    /// Text block index (0-based)
    pub text_block_index: usize,
    /// New translation text
    pub translation: Option<String>,
    /// New X position
    pub x: Option<f32>,
    /// New Y position
    pub y: Option<f32>,
    /// New width
    pub width: Option<f32>,
    /// New height
    pub height: Option<f32>,
    /// Font families (e.g. ["Arial", "Microsoft YaHei"])
    pub font_families: Option<Vec<String>>,
    /// Font size in pixels
    pub font_size: Option<f32>,
    /// Color as hex string (e.g. "#ff0000" or "#ff0000ff")
    pub color: Option<String>,
    /// Shader effect: "normal", "antique", "metal", "manga", "motionblur"
    pub shader_effect: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddTextBlockParams {
    /// Document index (0-based)
    pub index: usize,
    /// X position
    pub x: f32,
    /// Y position
    pub y: f32,
    /// Width
    pub width: f32,
    /// Height
    pub height: f32,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct RemoveTextBlockParams {
    /// Document index (0-based)
    pub index: usize,
    /// Text block index to remove (0-based)
    pub text_block_index: usize,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct MaskMorphParams {
    /// Document index (0-based)
    pub index: usize,
    /// Morphological radius in pixels (1-50)
    pub radius: u8,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct InpaintRegionParams {
    /// Document index (0-based)
    pub index: usize,
    /// Region X
    pub x: u32,
    /// Region Y
    pub y: u32,
    /// Region width
    pub width: u32,
    /// Region height
    pub height: u32,
}
