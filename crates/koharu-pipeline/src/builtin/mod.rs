mod aot_inpainting;
mod baberu_ocr;
mod flux2_klein;
mod font_detector;
mod koharu_layout_rfdetr_seg_2xl;
mod lama;
mod manga_ocr;
mod paddle_ocr_vl_1_6;
mod rorem_mixed;
mod translation;

pub use aot_inpainting::AotInpaintingConfig;
pub use baberu_ocr::BaberuOcrConfig;
pub use flux2_klein::Flux2KleinConfig;
pub use font_detector::FontDetectorConfig;
pub use koharu_layout_rfdetr_seg_2xl::KoharuLayoutRFDetrSeg2XLConfig;
pub use lama::{LaMaConfig, LaMaHDStrategy};
pub use manga_ocr::MangaOcrConfig;
pub use paddle_ocr_vl_1_6::PaddleOcrVl1_6Config;
pub use rorem_mixed::RoremMixedConfig;

use anyhow::{Context as _, Result};
use async_trait::async_trait;

use crate::{
    ConfiguredNode, DetectionModel, InpaintingModel, ModelRuntime, OcrModel, Processor,
    ProcessorFactory, TypographyModel,
};

pub(crate) struct BuiltinFactory;

#[async_trait]
impl ProcessorFactory for BuiltinFactory {
    async fn create(
        &self,
        node: &ConfiguredNode,
        device: koharu_ml::Device,
    ) -> Result<Box<dyn Processor>> {
        match node.spec().runtime {
            ModelRuntime::None => {}
            ModelRuntime::Diffusion => koharu_ml::init_diffusion()
                .await
                .context("failed to initialize the stable-diffusion.cpp runtime")?,
            ModelRuntime::Torch => koharu_ml::init_torch()
                .await
                .context("failed to initialize the LibTorch runtime")?,
            ModelRuntime::Llama => koharu_ml::init_llama()
                .await
                .context("failed to initialize the llama.cpp runtime")?,
        }
        Ok(match node {
            ConfiguredNode::Detection(DetectionModel::KoharuLayoutRFDetrSeg2XL(config)) => {
                Box::new(
                    koharu_layout_rfdetr_seg_2xl::KoharuLayoutRFDetrSeg2XLProcessor::load(
                        device, config,
                    )
                    .await?,
                )
            }
            ConfiguredNode::Ocr(OcrModel::MangaOcr(config)) => {
                Box::new(manga_ocr::MangaOcrProcessor::load(device, config).await?)
            }
            ConfiguredNode::Ocr(OcrModel::BaberuOcr(config)) => {
                Box::new(baberu_ocr::BaberuOcrProcessor::load(device, config).await?)
            }
            ConfiguredNode::Ocr(OcrModel::PaddleOcrVl1_6(config)) => {
                Box::new(paddle_ocr_vl_1_6::PaddleOcrVl1_6Processor::load(device, config).await?)
            }
            ConfiguredNode::Translation(config) => {
                Box::new(translation::TranslationProcessor::load(device, config).await?)
            }
            ConfiguredNode::Typography(TypographyModel::FontDetector(config)) => {
                Box::new(font_detector::FontDetectorProcessor::load(device, config).await?)
            }
            ConfiguredNode::Inpainting(InpaintingModel::LaMa(config)) => {
                Box::new(lama::LaMaProcessor::load(device, config).await?)
            }
            ConfiguredNode::Inpainting(InpaintingModel::AotInpainting(config)) => {
                Box::new(aot_inpainting::AotInpaintingProcessor::load(device, config).await?)
            }
            ConfiguredNode::Inpainting(InpaintingModel::Flux2Klein(config)) => {
                Box::new(flux2_klein::Flux2KleinProcessor::load(device, config).await?)
            }
            ConfiguredNode::Inpainting(InpaintingModel::RoremMixed(config)) => {
                Box::new(rorem_mixed::RoremMixedProcessor::load(device, config).await?)
            }
        })
    }
}
