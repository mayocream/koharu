use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PipelineConfig {
    pub detection: DetectionModel,
    pub segmentation: SegmentationModel,
    pub ocr: OcrModel,
    pub translation: TranslationModel,
    pub typography: TypographyModel,
    pub inpainting: InpaintingModel,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            detection: DetectionModel::PPDocLayoutV3(PPDocLayoutV3Config::default()),
            segmentation: SegmentationModel::MangaTextSegmentation(
                MangaTextSegmentationConfig::default(),
            ),
            ocr: OcrModel::PaddleOcrVl1_6(PaddleOcrVl1_6Config {}),
            translation: TranslationModel::Local(LocalTranslationConfig::default()),
            typography: TypographyModel::FontDetector(FontDetectorConfig::default()),
            inpainting: InpaintingModel::LaMa(LaMaConfig {}),
        }
    }
}

impl PipelineConfig {
    pub fn validate(&self) -> Result<()> {
        if let DetectionModel::PPDocLayoutV3(config) = &self.detection {
            unit("detection confidence", config.confidence)?;
        }
        match &self.segmentation {
            SegmentationModel::MangaTextSegmentation(config) => {
                unit("segmentation threshold", config.threshold)?;
                positive_optional("segmentation max_side", config.max_side)?;
            }
            SegmentationModel::SpeechBubbleSegmentation(config) => {
                unit_optional("bubble confidence", config.confidence)?;
                unit_optional("bubble NMS IoU", config.nms_iou)?;
            }
        }
        match &self.translation {
            TranslationModel::Local(config) => {
                non_empty("local translation model", &config.local_model)?;
                if config
                    .local_model
                    .parse::<koharu_translator::LocalModel>()
                    .is_err()
                {
                    bail!("unknown local translation model '{}'", config.local_model);
                }
            }
            TranslationModel::OpenAi(config)
            | TranslationModel::Gemini(config)
            | TranslationModel::Claude(config)
            | TranslationModel::DeepSeek(config) => validate_chat(config)?,
            TranslationModel::OpenAiCompatible(config) => {
                http_url("OpenAI-compatible base URL", &config.base_url)?;
                non_empty("remote translation model", &config.remote_model)?;
                finite_optional("translation temperature", config.temperature)?;
                positive_optional("translation max_tokens", config.max_tokens)?;
            }
            TranslationModel::DeepL(config) => {
                if let Some(url) = &config.base_url {
                    http_url("DeepL base URL", url)?;
                }
            }
            TranslationModel::GoogleCloudTranslation | TranslationModel::Caiyun => {}
        }
        let TypographyModel::FontDetector(config) = &self.typography;
        if config.top_k == 0 {
            bail!("font detector top_k must be positive");
        }
        if let InpaintingModel::AotInpainting(config) = &self.inpainting
            && config.max_side == 0
        {
            bail!("inpainting max_side must be positive");
        }
        Ok(())
    }
}

fn validate_chat(config: &ChatTranslationConfig) -> Result<()> {
    non_empty("remote translation model", &config.remote_model)?;
    finite_optional("translation temperature", config.temperature)?;
    positive_optional("translation max_tokens", config.max_tokens)
}

fn non_empty(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} cannot be empty");
    }
    Ok(())
}

fn http_url(name: &str, value: &Url) -> Result<()> {
    if !matches!(value.scheme(), "http" | "https") || value.host_str().is_none() {
        bail!("{name} must be an HTTP or HTTPS URL with a host");
    }
    Ok(())
}

fn unit(name: &str, value: f32) -> Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        bail!("{name} must be between zero and one");
    }
    Ok(())
}

fn unit_optional(name: &str, value: Option<f32>) -> Result<()> {
    if let Some(value) = value {
        unit(name, value)?;
    }
    Ok(())
}

fn finite_optional(name: &str, value: Option<f32>) -> Result<()> {
    if value.is_some_and(|value| !value.is_finite()) {
        bail!("{name} must be finite");
    }
    Ok(())
}

fn positive_optional<T>(name: &str, value: Option<T>) -> Result<()>
where
    T: Copy + Default + PartialOrd,
{
    if value.is_some_and(|value| value <= T::default()) {
        bail!("{name} must be positive");
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum DetectionModel {
    ComicTextDetector(ComicTextDetectorConfig),
    #[serde(rename = "pp_doclayout_v3")]
    PPDocLayoutV3(PPDocLayoutV3Config),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct ComicTextDetectorConfig {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct PPDocLayoutV3Config {
    pub confidence: f32,
}

impl Default for PPDocLayoutV3Config {
    fn default() -> Self {
        Self { confidence: 0.25 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum SegmentationModel {
    MangaTextSegmentation(MangaTextSegmentationConfig),
    SpeechBubbleSegmentation(SpeechBubbleSegmentationConfig),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct MangaTextSegmentationConfig {
    pub threshold: f32,
    pub max_side: Option<u32>,
    pub horizontal_flip: bool,
    pub vertical_flip: bool,
}

impl Default for MangaTextSegmentationConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            max_side: None,
            horizontal_flip: false,
            vertical_flip: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct SpeechBubbleSegmentationConfig {
    pub confidence: Option<f32>,
    pub nms_iou: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum OcrModel {
    #[serde(rename = "paddleocr_vl_1.6")]
    PaddleOcrVl1_6(PaddleOcrVl1_6Config),
    MangaOcr(MangaOcrConfig),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct PaddleOcrVl1_6Config {}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct MangaOcrConfig {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum TranslationModel {
    Local(LocalTranslationConfig),
    #[serde(rename = "openai")]
    OpenAi(ChatTranslationConfig),
    Gemini(ChatTranslationConfig),
    Claude(ChatTranslationConfig),
    #[serde(rename = "deepseek")]
    DeepSeek(ChatTranslationConfig),
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible(OpenAiCompatibleTranslationConfig),
    #[serde(rename = "deepl")]
    DeepL(DeepLTranslationConfig),
    GoogleCloudTranslation,
    Caiyun,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ChatTranslationConfig {
    pub remote_model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl Default for ChatTranslationConfig {
    fn default() -> Self {
        Self {
            remote_model: "gpt-4.1-mini".into(),
            temperature: None,
            max_tokens: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct LocalTranslationConfig {
    pub local_model: String,
}

impl Default for LocalTranslationConfig {
    fn default() -> Self {
        Self {
            local_model: "lfm2.5-1.2b-instruct".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct OpenAiCompatibleTranslationConfig {
    pub base_url: Url,
    pub remote_model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct DeepLTranslationConfig {
    pub base_url: Option<Url>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum TypographyModel {
    FontDetector(FontDetectorConfig),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct FontDetectorConfig {
    #[specta(type = f64)]
    pub top_k: usize,
}

impl Default for FontDetectorConfig {
    fn default() -> Self {
        Self { top_k: 3 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum InpaintingModel {
    #[serde(rename = "lama")]
    LaMa(LaMaConfig),
    AotInpainting(AotInpaintingConfig),
    Flux2Klein(Flux2KleinConfig),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct LaMaConfig {}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct Flux2KleinConfig {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct AotInpaintingConfig {
    pub max_side: u32,
}

impl Default for AotInpaintingConfig {
    fn default() -> Self {
        Self { max_side: 2048 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_select_one_model_for_every_stage() {
        let config = PipelineConfig::default();

        assert!(matches!(config.detection, DetectionModel::PPDocLayoutV3(_)));
        assert!(matches!(
            config.segmentation,
            SegmentationModel::MangaTextSegmentation(_)
        ));
        assert!(matches!(config.ocr, OcrModel::PaddleOcrVl1_6(_)));
        assert!(matches!(config.translation, TranslationModel::Local(_)));
        assert!(matches!(
            config.typography,
            TypographyModel::FontDetector(_)
        ));
        assert!(matches!(config.inpainting, InpaintingModel::LaMa(_)));
    }

    #[test]
    fn parses_one_model_per_stage() {
        let config: Wrapper = toml::from_str(
            r#"
                [pipeline.detection]
                model = "pp_doclayout_v3"
                confidence = 0.4

                [pipeline.segmentation]
                model = "manga_text_segmentation"

                [pipeline.ocr]
                model = "manga_ocr"

                [pipeline.translation]
                model = "openai"
                remote_model = "gpt-4.1-mini"

                [pipeline.typography]
                model = "font_detector"

                [pipeline.inpainting]
                model = "aot_inpainting"
            "#,
        )
        .unwrap();

        assert!(matches!(
            config.pipeline.detection,
            DetectionModel::PPDocLayoutV3(_)
        ));
        assert!(matches!(
            config.pipeline.translation,
            TranslationModel::OpenAi(_)
        ));
    }

    #[test]
    fn validates_every_configured_model_field() {
        let mut config = PipelineConfig {
            detection: DetectionModel::PPDocLayoutV3(PPDocLayoutV3Config::default()),
            segmentation: SegmentationModel::MangaTextSegmentation(
                MangaTextSegmentationConfig::default(),
            ),
            ocr: OcrModel::MangaOcr(MangaOcrConfig {}),
            translation: TranslationModel::OpenAi(ChatTranslationConfig::default()),
            typography: TypographyModel::FontDetector(FontDetectorConfig::default()),
            inpainting: InpaintingModel::AotInpainting(AotInpaintingConfig::default()),
        };
        config.validate().unwrap();

        config.detection = DetectionModel::PPDocLayoutV3(PPDocLayoutV3Config { confidence: 2.0 });
        assert!(config.validate().is_err());
        config.detection = PipelineConfig::default().detection;
        config.typography = TypographyModel::FontDetector(FontDetectorConfig { top_k: 0 });
        assert!(config.validate().is_err());
        config.typography = PipelineConfig::default().typography;
        config.translation = TranslationModel::Local(LocalTranslationConfig {
            local_model: "  ".into(),
        });
        assert!(config.validate().is_err());
        config.translation =
            TranslationModel::OpenAiCompatible(OpenAiCompatibleTranslationConfig {
                base_url: Url::parse("file:///tmp/model").unwrap(),
                remote_model: "model".into(),
                temperature: None,
                max_tokens: None,
            });
        assert!(config.validate().is_err());
    }

    #[derive(Deserialize)]
    struct Wrapper {
        pipeline: PipelineConfig,
    }
}
