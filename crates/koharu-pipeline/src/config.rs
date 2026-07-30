use serde::{Deserialize, Serialize};
use specta::Type;

use crate::stages::{Flux2KleinConfig, KoharuLayoutRFDetrSeg2XLConfig, RoremMixedConfig};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default, deny_unknown_fields)]
pub struct PipelineConfig {
    pub detection: DetectionModel,
    pub ocr: OcrModel,
    pub inpainting: InpaintingModel,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            detection: DetectionModel::KoharuLayoutRFDetrSeg2XL(
                KoharuLayoutRFDetrSeg2XLConfig::default(),
            ),
            ocr: OcrModel::PaddleOcrVl1_6,
            inpainting: InpaintingModel::LaMa {},
        }
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
    PaddleOcrVl1_6,
    #[serde(rename = "manga-ocr")]
    MangaOcr,
    #[serde(rename = "baberu-ocr")]
    BaberuOcr,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "model", deny_unknown_fields)]
pub enum InpaintingModel {
    #[serde(rename = "lama")]
    LaMa {},
    #[serde(rename = "aot-inpainting")]
    AotInpainting {},
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
        assert!(matches!(config.ocr, OcrModel::PaddleOcrVl1_6));
        assert!(matches!(config.inpainting, InpaintingModel::LaMa {}));
    }

    #[test]
    fn parses_phase_keyed_processor_configuration() {
        let config: PipelineConfig = toml::from_str(
            r#"
                [detection]
                model = "koharu-layout-rfdetr-seg-2xl"

                [ocr]
                model = "baberu-ocr"

                [inpainting]
                model = "rorem-mixed"
                prompt = "Remove the lettering."
                negative_prompt = "letters, words"
            "#,
        )
        .unwrap();

        assert!(matches!(
            config.detection,
            DetectionModel::KoharuLayoutRFDetrSeg2XL(_)
        ));
        assert!(matches!(config.ocr, OcrModel::BaberuOcr));
        assert!(matches!(
            config.inpainting,
            InpaintingModel::RoremMixed(config)
                if config.prompt == "Remove the lettering."
                    && config.negative_prompt == "letters, words"
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
        ));
    }

    #[test]
    fn rejects_removed_inpainting_options() {
        for (name, source) in [
            (
                "lama",
                r#"
                [inpainting]
                model = "lama"
                hd_strategy = "resize"
            "#,
            ),
            (
                "aot-inpainting",
                r#"
                [inpainting]
                model = "aot-inpainting"
                max_side = 1024
            "#,
            ),
            (
                "flux2-klein",
                r#"
                [inpainting]
                model = "flux2-klein"
                strength = 0.5
            "#,
            ),
            (
                "rorem-mixed",
                r#"
                [inpainting]
                model = "rorem-mixed"
                resolution = 1024
            "#,
            ),
        ] {
            assert!(
                toml::from_str::<PipelineConfig>(source).is_err(),
                "{name} accepted a removed option"
            );
        }
    }
}
