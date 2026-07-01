use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct AiInputImage {
    pub data_url: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct AiImageRequest {
    pub model: String,
    pub instructions: String,
    pub prompt: String,
    pub input_image: Option<AiInputImage>,
    pub quality: String,
    pub size: Option<String>,
    pub action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiImageResult {
    pub image_url: String,
}

#[async_trait]
pub trait AiImageProvider: Send + Sync {
    async fn generate_image(&self, request: AiImageRequest) -> anyhow::Result<AiImageResult>;
}

impl AiImageRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            instructions: "Generate or edit the requested image.".to_string(),
            prompt: prompt.into(),
            input_image: None,
            quality: "high".to_string(),
            size: None,
            action: None,
        }
    }

    pub fn with_input_image(mut self, data_url: impl Into<String>) -> Self {
        self.input_image = Some(AiInputImage {
            data_url: data_url.into(),
            detail: "high".to_string(),
        });
        self
    }
}
