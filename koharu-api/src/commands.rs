use koharu_types::{TextBlock, TextShaderEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WgpuDeviceInfo {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub ml_device: String,
    pub wgpu: WgpuDeviceInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenExternalPayload {
    pub url: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexPayload {
    pub index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailResult {
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocumentsPayload {
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileResult {
    pub filename: String,
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPayload {
    pub index: usize,
    pub text_block_index: Option<usize>,
    pub shader_effect: Option<TextShaderEffect>,
    pub font_family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTextBlocksPayload {
    pub index: usize,
    pub text_blocks: Vec<TextBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmListPayload {
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmLoadPayload {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmGeneratePayload {
    pub index: usize,
    pub text_block_index: Option<usize>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmLoadParams {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmGenerateParams {
    pub index: usize,
    pub text_block_index: Option<usize>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRequest {
    pub index: Option<usize>,
    pub llm_model_id: Option<String>,
    pub language: Option<String>,
    pub shader_effect: Option<TextShaderEffect>,
    pub font_family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InpaintRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInpaintMaskPayload {
    pub index: usize,
    #[serde(with = "serde_bytes")]
    pub mask: Vec<u8>,
    pub region: Option<InpaintRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBrushLayerPayload {
    pub index: usize,
    #[serde(with = "serde_bytes")]
    pub patch: Vec<u8>,
    pub region: InpaintRegion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InpaintPartialPayload {
    pub index: usize,
    pub region: InpaintRegion,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewImageParams {
    pub index: usize,
    pub layer: String,
    pub max_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewTextBlockParams {
    pub index: usize,
    pub text_block_index: usize,
    pub layer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocumentsParams {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportDocumentParams {
    pub index: usize,
    pub output_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderParams {
    pub index: usize,
    pub text_block_index: Option<usize>,
    pub shader_effect: Option<String>,
    pub font_family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessParams {
    pub index: Option<usize>,
    pub llm_model_id: Option<String>,
    pub language: Option<String>,
    pub shader_effect: Option<String>,
    pub font_family: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTextBlockPayload {
    pub index: usize,
    pub text_block_index: usize,
    pub translation: Option<String>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub font_families: Option<Vec<String>>,
    pub font_size: Option<f32>,
    pub color: Option<String>,
    pub shader_effect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddTextBlockPayload {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveTextBlockPayload {
    pub index: usize,
    pub text_block_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MaskMorphPayload {
    pub index: usize,
    pub radius: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InpaintRegionParams {
    pub index: usize,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[cfg(test)]
mod tests {
    use koharu_types::TextStyle;
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use super::*;

    fn round_trip<T>(value: &T)
    where
        T: Serialize + DeserializeOwned,
    {
        let encoded = serde_json::to_vec(value).expect("serialize");
        let decoded: T = serde_json::from_slice(&encoded).expect("deserialize");
        let original = serde_json::to_value(value).expect("serialize to value");
        let restored = serde_json::to_value(decoded).expect("serialize decoded to value");
        assert_eq!(original, restored);
    }

    #[test]
    fn command_dtos_round_trip() {
        let text_block = TextBlock {
            x: 10.0,
            y: 11.0,
            width: 120.0,
            height: 40.0,
            confidence: 0.95,
            text: Some("source".to_string()),
            translation: Some("translated".to_string()),
            style: Some(TextStyle {
                font_families: vec!["Noto Sans".to_string()],
                font_size: Some(18.0),
                color: [255, 255, 255, 255],
                effect: Some(TextShaderEffect::Manga),
            }),
            ..Default::default()
        };

        round_trip(&WgpuDeviceInfo {
            name: "Test".to_string(),
            backend: "Vulkan".to_string(),
            device_type: "DiscreteGpu".to_string(),
            driver: "test".to_string(),
            driver_info: "1.0".to_string(),
        });
        round_trip(&DeviceInfo {
            ml_device: "CPU".to_string(),
            wgpu: WgpuDeviceInfo {
                name: "Test".to_string(),
                backend: "Vulkan".to_string(),
                device_type: "DiscreteGpu".to_string(),
                driver: "test".to_string(),
                driver_info: "1.0".to_string(),
            },
        });
        round_trip(&OpenExternalPayload {
            url: "https://example.com".to_string(),
        });
        round_trip(&IndexPayload { index: 2 });
        round_trip(&ThumbnailResult {
            data: vec![1, 2, 3],
            content_type: "image/webp".to_string(),
        });
        round_trip(&FileEntry {
            name: "page.png".to_string(),
            data: vec![7, 8, 9],
        });
        round_trip(&OpenDocumentsPayload {
            files: vec![FileEntry {
                name: "page.png".to_string(),
                data: vec![7, 8, 9],
            }],
        });
        round_trip(&FileResult {
            filename: "page_koharu.png".to_string(),
            data: vec![1, 2, 3, 4],
            content_type: "image/png".to_string(),
        });
        round_trip(&RenderPayload {
            index: 1,
            text_block_index: Some(3),
            shader_effect: Some(TextShaderEffect::Manga),
            font_family: Some("Noto Sans".to_string()),
        });
        round_trip(&UpdateTextBlocksPayload {
            index: 1,
            text_blocks: vec![text_block.clone()],
        });
        round_trip(&LlmListPayload {
            language: Some("zh-CN".to_string()),
        });
        round_trip(&LlmLoadPayload {
            id: "sakura".to_string(),
        });
        round_trip(&LlmGeneratePayload {
            index: 1,
            text_block_index: Some(0),
            language: Some("zh-CN".to_string()),
        });
        round_trip(&LlmLoadParams {
            id: "sakura".to_string(),
        });
        round_trip(&LlmGenerateParams {
            index: 1,
            text_block_index: Some(0),
            language: Some("zh-CN".to_string()),
        });
        round_trip(&ProcessRequest {
            index: Some(1),
            llm_model_id: Some("sakura".to_string()),
            language: Some("zh-CN".to_string()),
            shader_effect: Some(TextShaderEffect::Manga),
            font_family: Some("Noto Sans".to_string()),
        });
        round_trip(&InpaintRegion {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        });
        round_trip(&UpdateInpaintMaskPayload {
            index: 1,
            mask: vec![0, 255],
            region: Some(InpaintRegion {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            }),
        });
        round_trip(&UpdateBrushLayerPayload {
            index: 1,
            patch: vec![1, 2, 3],
            region: InpaintRegion {
                x: 4,
                y: 5,
                width: 6,
                height: 7,
            },
        });
        round_trip(&InpaintPartialPayload {
            index: 1,
            region: InpaintRegion {
                x: 8,
                y: 9,
                width: 10,
                height: 11,
            },
        });
        round_trip(&ViewImageParams {
            index: 1,
            layer: "original".to_string(),
            max_size: Some(512),
        });
        round_trip(&ViewTextBlockParams {
            index: 1,
            text_block_index: 0,
            layer: Some("rendered".to_string()),
        });
        round_trip(&OpenDocumentsParams {
            paths: vec!["a.png".to_string(), "b.png".to_string()],
        });
        round_trip(&ExportDocumentParams {
            index: 1,
            output_path: "out.png".to_string(),
        });
        round_trip(&RenderParams {
            index: 1,
            text_block_index: Some(0),
            shader_effect: Some("manga".to_string()),
            font_family: Some("Noto Sans".to_string()),
        });
        round_trip(&ProcessParams {
            index: Some(1),
            llm_model_id: Some("sakura".to_string()),
            language: Some("zh-CN".to_string()),
            shader_effect: Some("manga".to_string()),
            font_family: Some("Noto Sans".to_string()),
        });
        round_trip(&UpdateTextBlockPayload {
            index: 1,
            text_block_index: 0,
            translation: Some("translated".to_string()),
            x: Some(1.0),
            y: Some(2.0),
            width: Some(3.0),
            height: Some(4.0),
            font_families: Some(vec!["Noto Sans".to_string()]),
            font_size: Some(16.0),
            color: Some("#ffffff".to_string()),
            shader_effect: Some("manga".to_string()),
        });
        round_trip(&AddTextBlockPayload {
            index: 1,
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        });
        round_trip(&RemoveTextBlockPayload {
            index: 1,
            text_block_index: 0,
        });
        round_trip(&MaskMorphPayload {
            index: 1,
            radius: 2,
        });
        round_trip(&InpaintRegionParams {
            index: 1,
            x: 2,
            y: 3,
            width: 4,
            height: 5,
        });
    }
}
