mod aot_inpainting;
mod comic_text_detector;
mod flux2_klein;
mod font_detector;
mod lama;
mod manga_ocr;
mod manga_text_segmentation;
mod paddle_ocr_vl_1_6;
mod pp_doclayout_v3;
mod speech_bubble_segmentation;
mod translation;

use anyhow::{Context as _, Result};
use async_trait::async_trait;

use crate::{
    ConfiguredModel, DetectionModel, InpaintingModel, OcrModel, Processor, ProcessorFactory,
    SegmentationModel, TranslationModel, TypographyModel,
};

pub(crate) struct BuiltinFactory;

#[async_trait]
impl ProcessorFactory for BuiltinFactory {
    async fn load(
        &self,
        model: &ConfiguredModel,
        device: koharu_ml::Device,
    ) -> Result<Box<dyn Processor>> {
        match model {
            ConfiguredModel::Ocr(OcrModel::PaddleOcrVl1_6(_))
            | ConfiguredModel::Translation(TranslationModel::Local(_)) => koharu_ml::init_llama()
                .await
                .context("failed to initialize the llama.cpp runtime")?,
            ConfiguredModel::Inpainting(InpaintingModel::Flux2Klein(_)) => {
                koharu_ml::init_diffusion()
                    .await
                    .context("failed to initialize the stable-diffusion.cpp runtime")?
            }
            ConfiguredModel::Translation(_) => {}
            _ => koharu_ml::init_torch()
                .await
                .context("failed to initialize the LibTorch runtime")?,
        }
        Ok(match model {
            ConfiguredModel::Detection(DetectionModel::ComicTextDetector(config)) => Box::new(
                comic_text_detector::ComicTextDetectorProcessor::load(device, config).await?,
            ),
            ConfiguredModel::Detection(DetectionModel::PPDocLayoutV3(config)) => {
                Box::new(pp_doclayout_v3::PPDocLayoutV3Processor::load(device, config).await?)
            }
            ConfiguredModel::Segmentation(SegmentationModel::MangaTextSegmentation(config)) => {
                Box::new(
                    manga_text_segmentation::MangaTextSegmentationProcessor::load(device, config)
                        .await?,
                )
            }
            ConfiguredModel::Segmentation(SegmentationModel::SpeechBubbleSegmentation(config)) => {
                Box::new(
                    speech_bubble_segmentation::SpeechBubbleSegmentationProcessor::load(
                        device, config,
                    )
                    .await?,
                )
            }
            ConfiguredModel::Ocr(OcrModel::MangaOcr(config)) => {
                Box::new(manga_ocr::MangaOcrProcessor::load(device, config).await?)
            }
            ConfiguredModel::Ocr(OcrModel::PaddleOcrVl1_6(config)) => {
                Box::new(paddle_ocr_vl_1_6::PaddleOcrVl1_6Processor::load(device, config).await?)
            }
            ConfiguredModel::Translation(config) => {
                Box::new(translation::TranslationProcessor::load(device, config).await?)
            }
            ConfiguredModel::Typography(TypographyModel::FontDetector(config)) => {
                Box::new(font_detector::FontDetectorProcessor::load(device, config).await?)
            }
            ConfiguredModel::Inpainting(InpaintingModel::LaMa(config)) => {
                Box::new(lama::LaMaProcessor::load(device, config).await?)
            }
            ConfiguredModel::Inpainting(InpaintingModel::AotInpainting(config)) => {
                Box::new(aot_inpainting::AotInpaintingProcessor::load(device, config).await?)
            }
            ConfiguredModel::Inpainting(InpaintingModel::Flux2Klein(config)) => {
                Box::new(flux2_klein::Flux2KleinProcessor::load(device, config).await?)
            }
        })
    }
}
