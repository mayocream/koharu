use serde::{Deserialize, Serialize};
use specta::Type;

use crate::builtin::{
    AotInpaintingConfig, BaberuOcrConfig, ComicLayoutYolo26sConfig, ComicOnomatopoeiaConfig,
    ComicTextDetectorConfig, Flux2KleinConfig, FontDetectorConfig, LaMaConfig, MangaOcrConfig,
    MangaTextMaskConfig, MaskFusionConfig, PPDocLayoutV3Config, PaddleOcrVl1_6Config,
    RoremMixedConfig, Yolo11nSpeechBubbleConfig, YoloV8mSpeechBubbleConfig,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct PipelineConfig {
    pub processors: Vec<ProcessorConfig>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            processors: vec![
                ProcessorConfig::PPDocLayoutV3(PPDocLayoutV3Config::default()),
                ProcessorConfig::ComicLayoutYolo26s(ComicLayoutYolo26sConfig::default()),
                ProcessorConfig::MangaTextMask(MangaTextMaskConfig::default()),
                ProcessorConfig::ComicOnomatopoeia(ComicOnomatopoeiaConfig::default()),
                ProcessorConfig::MaskFusion(MaskFusionConfig::default()),
                ProcessorConfig::PaddleOcrVl1_6(PaddleOcrVl1_6Config::default()),
                ProcessorConfig::FontDetector(FontDetectorConfig::default()),
                ProcessorConfig::LaMa(LaMaConfig::default()),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum ProcessorConfig {
    ComicTextDetector(ComicTextDetectorConfig),
    #[serde(rename = "pp_doclayout_v3")]
    PPDocLayoutV3(PPDocLayoutV3Config),
    ComicLayoutYolo26s(ComicLayoutYolo26sConfig),
    MangaTextMask(MangaTextMaskConfig),
    #[serde(rename = "speech_bubble_yolov8m")]
    SpeechBubbleYoloV8m(YoloV8mSpeechBubbleConfig),
    #[serde(rename = "speech_bubble_yolo11n")]
    SpeechBubbleYolo11n(Yolo11nSpeechBubbleConfig),
    ComicOnomatopoeia(ComicOnomatopoeiaConfig),
    MaskFusion(MaskFusionConfig),
    #[serde(rename = "paddleocr_vl_1.6")]
    PaddleOcrVl1_6(PaddleOcrVl1_6Config),
    MangaOcr(MangaOcrConfig),
    BaberuOcr(BaberuOcrConfig),
    FontDetector(FontDetectorConfig),
    #[serde(rename = "lama")]
    LaMa(LaMaConfig),
    AotInpainting(AotInpaintingConfig),
    Flux2Klein(Flux2KleinConfig),
    RoremMixed(RoremMixedConfig),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_compose_specialized_processors() {
        let config = PipelineConfig::default();

        assert!(
            config
                .processors
                .iter()
                .any(|processor| matches!(processor, ProcessorConfig::ComicLayoutYolo26s(_)))
        );
        assert!(
            config
                .processors
                .iter()
                .any(|processor| matches!(processor, ProcessorConfig::ComicOnomatopoeia(_)))
        );
        assert!(
            config
                .processors
                .iter()
                .any(|processor| matches!(processor, ProcessorConfig::MaskFusion(_)))
        );
    }

    #[test]
    fn parses_an_explicit_processor_list() {
        let config: PipelineConfig = toml::from_str(
            r#"
                [[processors]]
                model = "manga_text_mask"
                threshold = 0.4

                [[processors]]
                model = "mask_fusion"
                coo_padding = 3

                [[processors]]
                model = "rorem_mixed"
                resolution = 1024
                mask_dilation = 20

                [[processors]]
                model = "speech_bubble_yolo11n"
                confidence = 0.3
                nms_iou = 0.7
            "#,
        )
        .unwrap();

        assert_eq!(config.processors.len(), 4);
        assert!(matches!(
            &config.processors[0],
            ProcessorConfig::MangaTextMask(config) if config.threshold == 0.4
        ));
        assert!(matches!(
            &config.processors[2],
            ProcessorConfig::RoremMixed(config)
                if config.resolution == 1024
                    && config.mask_dilation == 20
                    && config.num_inference_steps == 30
        ));
        assert!(matches!(
            &config.processors[3],
            ProcessorConfig::SpeechBubbleYolo11n(config)
                if config.confidence == Some(0.3) && config.nms_iou == Some(0.7)
        ));
    }
}
