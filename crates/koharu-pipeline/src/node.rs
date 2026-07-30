use koharu_translator::Providers;
use serde::{Deserialize, Serialize};

use crate::{DetectionModel, InpaintingModel, OcrModel, Phase, ProcessorId, TypographyModel};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) enum ConfiguredNode {
    Detection(DetectionModel),
    Ocr(OcrModel),
    Translation(Providers),
    Typography(TypographyModel),
    Inpainting(InpaintingModel),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModelRuntime {
    None,
    Torch,
    Llama,
    Diffusion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NodeSpec {
    pub id: ProcessorId,
    pub name: &'static str,
    pub phase: Phase,
    pub runtime: ModelRuntime,
    pub supports_element_scope: bool,
}

impl ConfiguredNode {
    pub(crate) const fn spec(&self) -> NodeSpec {
        let (id, name, phase, runtime, supports_element_scope) = match self {
            Self::Detection(DetectionModel::KoharuLayoutRFDetrSeg2XL(_)) => (
                ProcessorId::KoharuLayoutRFDetrSeg2XL,
                "Koharu Layout RF-DETR Seg 2XL",
                Phase::Detection,
                ModelRuntime::Torch,
                false,
            ),
            Self::Ocr(OcrModel::PaddleOcrVl1_6(_)) => (
                ProcessorId::PaddleOcrVl1_6,
                "PaddleOCR-VL 1.6",
                Phase::Ocr,
                ModelRuntime::Torch,
                true,
            ),
            Self::Ocr(OcrModel::MangaOcr(_)) => (
                ProcessorId::MangaOcr,
                "MangaOcr",
                Phase::Ocr,
                ModelRuntime::Torch,
                true,
            ),
            Self::Ocr(OcrModel::BaberuOcr(_)) => (
                ProcessorId::BaberuOcr,
                "BaberuOcr",
                Phase::Ocr,
                ModelRuntime::Torch,
                true,
            ),
            Self::Translation(Providers::Local(_)) => (
                ProcessorId::Translation,
                "LocalTranslator",
                Phase::Translation,
                ModelRuntime::Llama,
                true,
            ),
            Self::Translation(Providers::OpenAi(_)) => (
                ProcessorId::Translation,
                "OpenAI",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Translation(Providers::Gemini(_)) => (
                ProcessorId::Translation,
                "Gemini",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Translation(Providers::Claude(_)) => (
                ProcessorId::Translation,
                "Claude",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Translation(Providers::DeepSeek(_)) => (
                ProcessorId::Translation,
                "DeepSeek",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Translation(Providers::OpenAiCompatible(_)) => (
                ProcessorId::Translation,
                "OpenAI-compatible",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Translation(Providers::OpenRouter(_)) => (
                ProcessorId::Translation,
                "OpenRouter",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Translation(Providers::LmStudio(_)) => (
                ProcessorId::Translation,
                "LM Studio",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Translation(Providers::DeepL(_)) => (
                ProcessorId::Translation,
                "DeepL",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Translation(Providers::GoogleCloudTranslation(_)) => (
                ProcessorId::Translation,
                "Google Cloud Translation",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Translation(Providers::Caiyun(_)) => (
                ProcessorId::Translation,
                "Caiyun",
                Phase::Translation,
                ModelRuntime::None,
                true,
            ),
            Self::Typography(TypographyModel::FontDetector(_)) => (
                ProcessorId::FontDetector,
                "FontDetector",
                Phase::Typography,
                ModelRuntime::Torch,
                true,
            ),
            Self::Inpainting(InpaintingModel::LaMa(_)) => (
                ProcessorId::LaMa,
                "LaMa",
                Phase::Inpainting,
                ModelRuntime::Torch,
                false,
            ),
            Self::Inpainting(InpaintingModel::AotInpainting(_)) => (
                ProcessorId::AotInpainting,
                "AotInpainting",
                Phase::Inpainting,
                ModelRuntime::Torch,
                false,
            ),
            Self::Inpainting(InpaintingModel::Flux2Klein(_)) => (
                ProcessorId::Flux2Klein,
                "FLUX.2 Klein",
                Phase::Inpainting,
                ModelRuntime::Diffusion,
                false,
            ),
            Self::Inpainting(InpaintingModel::RoremMixed(_)) => (
                ProcessorId::RoremMixed,
                "RORem Mixed",
                Phase::Inpainting,
                ModelRuntime::Diffusion,
                false,
            ),
        };
        NodeSpec {
            id,
            name,
            phase,
            runtime,
            supports_element_scope,
        }
    }

    pub(crate) const fn name(&self) -> &'static str {
        self.spec().name
    }

    pub(crate) const fn uses_accelerator(&self) -> bool {
        !matches!(self.spec().runtime, ModelRuntime::None)
    }
}
