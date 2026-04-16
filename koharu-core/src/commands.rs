use crate::{TextShaderEffect, TextStrokeStyle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub ml_device: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenExternalPayload {
    pub url: String,
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
pub struct LlmCatalogPayload {
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmLoadParams {
    pub target: crate::LlmTarget,
    pub options: Option<crate::LlmGenerationOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LlmGenerateParams {
    pub document_id: String,
    pub text_block_index: Option<usize>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRequest {
    pub document_id: Option<String>,
    pub llm: Option<crate::PipelineLlmRequest>,
    pub language: Option<String>,
    pub system_prompt: Option<String>,
    pub shader_effect: Option<TextShaderEffect>,
    pub shader_stroke: Option<TextStrokeStyle>,
    pub default_font: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewImageParams {
    pub document_id: String,
    pub layer: String,
    pub max_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewTextBlockParams {
    pub document_id: String,
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
    pub document_id: String,
    pub output_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderParams {
    pub document_id: String,
    pub text_block_index: Option<usize>,
    pub shader_effect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentIdParam {
    pub document_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentIndexParam {
    pub index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessParams {
    pub document_id: Option<String>,
    pub llm_target: Option<crate::LlmTarget>,
    pub language: Option<String>,
    pub system_prompt: Option<String>,
    pub shader_effect: Option<String>,
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
    pub document_id: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[cfg(test)]
mod tests {
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
        round_trip(&DeviceInfo {
            ml_device: "CPU".to_string(),
        });
        round_trip(&OpenExternalPayload {
            url: "https://example.com".to_string(),
        });

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
        round_trip(&LlmCatalogPayload {
            language: Some("zh-CN".to_string()),
        });
        round_trip(&crate::LlmLoadRequest {
            target: crate::LlmTarget {
                kind: crate::LlmTargetKind::Local,
                model_id: "sakura".to_string(),
                provider_id: None,
            },
            options: Some(crate::LlmGenerationOptions {
                temperature: Some(0.1),
                max_tokens: Some(1000),
                custom_system_prompt: None,
            }),
        });
        round_trip(&LlmLoadParams {
            target: crate::LlmTarget {
                kind: crate::LlmTargetKind::Provider,
                model_id: "gpt-5-mini".to_string(),
                provider_id: Some("openai".to_string()),
            },
            options: Some(crate::LlmGenerationOptions {
                temperature: None,
                max_tokens: None,
                custom_system_prompt: None,
            }),
        });
        round_trip(&LlmGenerateParams {
            document_id: "abc123".to_string(),
            text_block_index: Some(0),
            language: Some("zh-CN".to_string()),
        });
        round_trip(&ProcessRequest {
            document_id: Some("abc123".to_string()),
            llm: Some(crate::PipelineLlmRequest {
                target: crate::LlmTarget {
                    kind: crate::LlmTargetKind::Provider,
                    model_id: "gpt-5-mini".to_string(),
                    provider_id: Some("openai".to_string()),
                },
                options: Some(crate::LlmGenerationOptions {
                    temperature: Some(0.1),
                    max_tokens: Some(1000),
                    custom_system_prompt: Some("Translate manga".to_string()),
                }),
            }),
            language: Some("zh-CN".to_string()),
            system_prompt: Some("Translate manga".to_string()),
            shader_effect: Some(TextShaderEffect {
                italic: true,
                bold: true,
            }),
            shader_stroke: Some(TextStrokeStyle {
                enabled: false,
                color: [255, 255, 255, 255],
                width_px: Some(2.0),
            }),
            default_font: None,
        });
        round_trip(&ViewImageParams {
            document_id: "abc123".to_string(),
            layer: "original".to_string(),
            max_size: Some(512),
        });
        round_trip(&ViewTextBlockParams {
            document_id: "abc123".to_string(),
            text_block_index: 0,
            layer: Some("rendered".to_string()),
        });
        round_trip(&OpenDocumentsParams {
            paths: vec!["a.png".to_string(), "b.png".to_string()],
        });
        round_trip(&ExportDocumentParams {
            document_id: "abc123".to_string(),
            output_path: "out.png".to_string(),
        });
        round_trip(&RenderParams {
            document_id: "abc123".to_string(),
            text_block_index: Some(0),
            shader_effect: Some("bold".to_string()),
        });
        round_trip(&ProcessParams {
            document_id: Some("abc123".to_string()),
            llm_target: Some(crate::LlmTarget {
                kind: crate::LlmTargetKind::Local,
                model_id: "sakura".to_string(),
                provider_id: None,
            }),
            language: Some("zh-CN".to_string()),
            system_prompt: Some("Translate manga".to_string()),
            shader_effect: Some("italic,bold".to_string()),
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
            shader_effect: Some("italic,bold".to_string()),
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
            document_id: "abc123".to_string(),
            x: 2,
            y: 3,
            width: 4,
            height: 5,
        });
    }
}
