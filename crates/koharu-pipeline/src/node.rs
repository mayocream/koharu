use koharu_translator::Providers;

use crate::{DetectionModel, InpaintingModel, OcrModel, Stage};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ConfiguredNode {
    Detection(DetectionModel),
    Ocr(OcrModel),
    Translation(Providers),
    Inpainting(InpaintingModel),
}

impl ConfiguredNode {
    pub(crate) fn stage(&self) -> Stage {
        match self {
            Self::Detection(_) => Stage::Detection,
            Self::Ocr(_) => Stage::Ocr,
            Self::Translation(_) => Stage::Translation,
            Self::Inpainting(_) => Stage::Inpainting,
        }
    }

    pub(crate) fn model(&self) -> String {
        match self {
            Self::Detection(DetectionModel::KoharuLayoutRFDetrSeg2XL(_)) => {
                "koharu-layout-rfdetr-seg-2xl".to_owned()
            }
            Self::Ocr(OcrModel::PaddleOcrVl1_6(_)) => "paddleocr-vl-1.6".to_owned(),
            Self::Ocr(OcrModel::MangaOcr(_)) => "manga-ocr".to_owned(),
            Self::Ocr(OcrModel::BaberuOcr(_)) => "baberu-ocr".to_owned(),
            Self::Translation(provider) => provider_name(provider).to_owned(),
            Self::Inpainting(InpaintingModel::LaMa(_)) => "lama".to_owned(),
            Self::Inpainting(InpaintingModel::AotInpainting(_)) => "aot-inpainting".to_owned(),
            Self::Inpainting(InpaintingModel::Flux2Klein(_)) => "flux2-klein".to_owned(),
            Self::Inpainting(InpaintingModel::RoremMixed(_)) => "rorem-mixed".to_owned(),
        }
    }

    pub(crate) fn local(&self) -> bool {
        !matches!(self, Self::Translation(provider) if !matches!(provider, Providers::Local(_)))
    }
}

fn provider_name(provider: &Providers) -> &'static str {
    match provider {
        Providers::Local(_) => "local-translation",
        Providers::OpenAi(_) => "openai",
        Providers::Gemini(_) => "gemini",
        Providers::Claude(_) => "claude",
        Providers::DeepSeek(_) => "deepseek",
        Providers::OpenAiCompatible(_) => "openai-compatible",
        Providers::OpenRouter(_) => "openrouter",
        Providers::LmStudio(_) => "lm-studio",
        Providers::DeepL(_) => "deepl",
        Providers::GoogleCloudTranslation(_) => "google-cloud-translation",
        Providers::Caiyun(_) => "caiyun",
    }
}
