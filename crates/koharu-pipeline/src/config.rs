use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::builtin::{
    AotInpaintingConfig, BaberuOcrConfig, Flux2KleinConfig, FontDetectorConfig,
    KoharuLayoutRFDetrSeg2XLConfig, LaMaConfig, MangaOcrConfig, PaddleOcrVl1_6Config,
    RoremMixedConfig,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct PipelineConfig {
    pub detection: DetectionModel,
    pub ocr: OcrModel,
    pub typography: TypographyModel,
    pub inpainting: InpaintingModel,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            detection: DetectionModel::KoharuLayoutRFDetrSeg2XL(
                KoharuLayoutRFDetrSeg2XLConfig::default(),
            ),
            ocr: OcrModel::PaddleOcrVl1_6(PaddleOcrVl1_6Config::default()),
            typography: TypographyModel::FontDetector(FontDetectorConfig::default()),
            inpainting: InpaintingModel::LaMa(LaMaConfig::default()),
        }
    }
}

impl PipelineConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        let DetectionModel::KoharuLayoutRFDetrSeg2XL(detection) = &self.detection;
        for (name, value) in [
            ("text", detection.text_threshold),
            ("bubble", detection.bubble_threshold),
            ("panel", detection.panel_threshold),
        ] {
            if let Some(value) = value {
                ensure!(
                    value.is_finite() && (0.0..=1.0).contains(&value),
                    "{name} confidence threshold must be finite and between zero and one"
                );
            }
        }
        let TypographyModel::FontDetector(typography) = &self.typography;
        ensure!(
            typography.top_k > 0,
            "font detector top_k must be greater than zero"
        );
        match &self.inpainting {
            InpaintingModel::LaMa(config) => {
                ensure!(
                    config.hd_strategy_crop_trigger_size > 0,
                    "LaMa crop trigger must be positive"
                );
                ensure!(
                    config.hd_strategy_resize_limit > 0,
                    "LaMa resize limit must be positive"
                );
            }
            InpaintingModel::AotInpainting(config) => {
                ensure!(
                    config.max_side > 0,
                    "AOT max_side must be greater than zero"
                );
            }
            InpaintingModel::Flux2Klein(config) => {
                ensure!(!config.prompt.contains('\0'), "FLUX.2 prompt contains NUL");
                ensure!(
                    config.strength.is_finite() && config.strength > 0.0 && config.strength <= 1.0,
                    "FLUX.2 strength must be finite and in (0, 1]"
                );
                ensure!(
                    config.num_inference_steps > 0,
                    "FLUX.2 inference steps must be positive"
                );
            }
            InpaintingModel::RoremMixed(config) => {
                ensure!(
                    matches!(config.resolution, 512 | 1024),
                    "RORem resolution must be 512 or 1024"
                );
                ensure!(
                    config.num_inference_steps > 0,
                    "RORem inference steps must be positive"
                );
                ensure!(
                    config.guidance_scale.is_finite() && config.guidance_scale > 0.0,
                    "RORem guidance must be finite and positive"
                );
                ensure!(
                    config.strength.is_finite() && config.strength > 0.0 && config.strength < 1.0,
                    "RORem strength must be finite and in (0, 1)"
                );
                ensure!(
                    !config.prompt.contains('\0') && !config.negative_prompt.contains('\0'),
                    "RORem prompt contains NUL"
                );
            }
        }
        Ok(())
    }

    pub(crate) fn nodes(
        &self,
        translation: &koharu_translator::TranslationConfig,
    ) -> [crate::ConfiguredNode; 5] {
        [
            crate::ConfiguredNode::Detection(self.detection.clone()),
            crate::ConfiguredNode::Ocr(self.ocr.clone()),
            crate::ConfiguredNode::Translation(translation.model.clone()),
            crate::ConfiguredNode::Typography(self.typography.clone()),
            crate::ConfiguredNode::Inpainting(self.inpainting.clone()),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", deny_unknown_fields)]
pub enum DetectionModel {
    #[serde(rename = "koharu-layout-rfdetr-seg-2xl")]
    KoharuLayoutRFDetrSeg2XL(KoharuLayoutRFDetrSeg2XLConfig),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", deny_unknown_fields)]
pub enum OcrModel {
    #[serde(rename = "paddleocr-vl-1.6")]
    PaddleOcrVl1_6(PaddleOcrVl1_6Config),
    #[serde(rename = "manga-ocr")]
    MangaOcr(MangaOcrConfig),
    #[serde(rename = "baberu-ocr")]
    BaberuOcr(BaberuOcrConfig),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", deny_unknown_fields)]
pub enum TypographyModel {
    #[serde(rename = "font-detector")]
    FontDetector(FontDetectorConfig),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", deny_unknown_fields)]
pub enum InpaintingModel {
    #[serde(rename = "lama")]
    LaMa(LaMaConfig),
    #[serde(rename = "aot-inpainting")]
    AotInpainting(AotInpaintingConfig),
    #[serde(rename = "flux2-klein")]
    Flux2Klein(Flux2KleinConfig),
    #[serde(rename = "rorem-mixed")]
    RoremMixed(RoremMixedConfig),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_select_one_processor_for_each_phase() {
        let config = PipelineConfig::default();

        assert!(matches!(
            config.detection,
            DetectionModel::KoharuLayoutRFDetrSeg2XL(_)
        ));
        assert!(matches!(config.ocr, OcrModel::PaddleOcrVl1_6(_)));
        assert!(matches!(
            config.typography,
            TypographyModel::FontDetector(_)
        ));
        assert!(matches!(config.inpainting, InpaintingModel::LaMa(_)));
    }

    #[test]
    fn parses_phase_keyed_processor_configuration() {
        let config: PipelineConfig = toml::from_str(
            r#"
                [detection]
                model = "koharu-layout-rfdetr-seg-2xl"

                [ocr]
                model = "baberu-ocr"

                [typography]
                model = "font-detector"
                top_k = 5

                [inpainting]
                model = "rorem-mixed"
                resolution = 1024
                mask_dilation = 20
            "#,
        )
        .unwrap();

        assert!(matches!(
            config.detection,
            DetectionModel::KoharuLayoutRFDetrSeg2XL(_)
        ));
        assert!(matches!(config.ocr, OcrModel::BaberuOcr(_)));
        assert!(matches!(
            config.typography,
            TypographyModel::FontDetector(config) if config.top_k == 5
        ));
        assert!(matches!(
            config.inpainting,
            InpaintingModel::RoremMixed(config)
                if config.resolution == 1024
                    && config.mask_dilation == 20
                    && config.num_inference_steps == 30
        ));
    }

    #[test]
    fn missing_slots_use_defaults() {
        let config = toml::from_str::<PipelineConfig>("").unwrap();

        assert_eq!(config, PipelineConfig::default());
    }

    #[test]
    fn rejects_legacy_processor_configuration() {
        let result = toml::from_str::<PipelineConfig>(
            r#"
                [[processors]]
                model = "comic_layout_yolo26s"
                enabled = false

                [[processors]]
                model = "mask_fusion"
            "#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_unknown_model_configuration_fields() {
        let result = toml::from_str::<PipelineConfig>(
            r#"
                [detection]
                model = "koharu-layout-rfdetr-seg-2xl"
                legacy_threshold = 0.5

                [ocr]
                model = "paddleocr-vl-1.6"
                legacy_language = "ja"

                [typography]
                model = "font-detector"
                top_k = 5
                legacy_fonts = ["Example"]

                [inpainting]
                model = "lama"
                legacy_resolution = 1024
            "#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn parses_detection_and_generative_inpainting_options() {
        let config = toml::from_str::<PipelineConfig>(
            r#"
                [detection]
                model = "koharu-layout-rfdetr-seg-2xl"
                text_threshold = 0.25
                bubble_threshold = 0.45
                panel_threshold = 0.55

                [inpainting]
                model = "flux2-klein"
                prompt = "Reconstruct the illustration without text."
                padding_mask_crop = 64
                strength = 0.75
                num_inference_steps = 8
                seed = 42
            "#,
        )
        .unwrap();

        assert!(matches!(
            config.detection,
            DetectionModel::KoharuLayoutRFDetrSeg2XL(config)
                if config.text_threshold == Some(0.25)
                    && config.bubble_threshold == Some(0.45)
                    && config.panel_threshold == Some(0.55)
        ));
        assert!(matches!(
            config.inpainting,
            InpaintingModel::Flux2Klein(config)
                if config.prompt == "Reconstruct the illustration without text."
                    && config.padding_mask_crop == Some(64)
                    && config.strength == 0.75
                    && config.num_inference_steps == 8
                    && config.seed == 42
        ));
    }
}
