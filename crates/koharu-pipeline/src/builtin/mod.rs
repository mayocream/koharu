mod aot_inpainting;
mod baberu_ocr;
mod comic_layout_yolo26s;
mod comic_onomatopoeia;
mod comic_text_detector;
mod flux2_klein;
mod font_detector;
mod lama;
mod manga_ocr;
mod manga_text_mask;
mod mask_fusion;
mod paddle_ocr_vl_1_6;
mod pp_doclayout_v3;
mod rorem_mixed;
mod speech_bubble_yolo11n;
mod speech_bubble_yolov8m;
mod translation;

pub use aot_inpainting::AotInpaintingConfig;
pub use baberu_ocr::BaberuOcrConfig;
pub use comic_layout_yolo26s::ComicLayoutYolo26sConfig;
pub use comic_onomatopoeia::ComicOnomatopoeiaConfig;
pub use comic_text_detector::ComicTextDetectorConfig;
pub use flux2_klein::Flux2KleinConfig;
pub use font_detector::FontDetectorConfig;
pub use lama::LaMaConfig;
pub use manga_ocr::MangaOcrConfig;
pub use manga_text_mask::MangaTextMaskConfig;
pub use mask_fusion::MaskFusionConfig;
pub use paddle_ocr_vl_1_6::PaddleOcrVl1_6Config;
pub use pp_doclayout_v3::PPDocLayoutV3Config;
pub use rorem_mixed::RoremMixedConfig;
pub use speech_bubble_yolo11n::Yolo11nSpeechBubbleConfig;
pub use speech_bubble_yolov8m::YoloV8mSpeechBubbleConfig;

use anyhow::{Context as _, Result};
use async_trait::async_trait;

use crate::{ConfiguredModel, Processor, ProcessorConfig, ProcessorFactory, plan::ModelRuntime};

pub(crate) struct BuiltinFactory;

#[async_trait]
impl ProcessorFactory for BuiltinFactory {
    async fn create(
        &self,
        model: &ConfiguredModel,
        device: koharu_ml::Device,
    ) -> Result<Box<dyn Processor>> {
        match model.runtime() {
            ModelRuntime::Llama => koharu_ml::init_llama()
                .await
                .context("failed to initialize the llama.cpp runtime")?,
            ModelRuntime::Diffusion => koharu_ml::init_diffusion()
                .await
                .context("failed to initialize the stable-diffusion.cpp runtime")?,
            ModelRuntime::Torch => koharu_ml::init_torch()
                .await
                .context("failed to initialize the LibTorch runtime")?,
            ModelRuntime::None => {}
        }
        Ok(match model {
            ConfiguredModel::Processor(ProcessorConfig::ComicTextDetector(config)) => Box::new(
                comic_text_detector::ComicTextDetectorProcessor::load(device, config).await?,
            ),
            ConfiguredModel::Processor(ProcessorConfig::PPDocLayoutV3(config)) => {
                Box::new(pp_doclayout_v3::PPDocLayoutV3Processor::load(device, config).await?)
            }
            ConfiguredModel::Processor(ProcessorConfig::ComicLayoutYolo26s(config)) => Box::new(
                comic_layout_yolo26s::ComicLayoutYolo26sProcessor::load(device, config).await?,
            ),
            ConfiguredModel::Processor(ProcessorConfig::MangaTextMask(config)) => {
                Box::new(manga_text_mask::MangaTextMaskProcessor::load(device, config).await?)
            }
            ConfiguredModel::Processor(ProcessorConfig::SpeechBubbleYoloV8m(config)) => Box::new(
                speech_bubble_yolov8m::YoloV8mSpeechBubbleProcessor::load(device, config).await?,
            ),
            ConfiguredModel::Processor(ProcessorConfig::SpeechBubbleYolo11n(config)) => Box::new(
                speech_bubble_yolo11n::Yolo11nSpeechBubbleProcessor::load(device, config).await?,
            ),
            ConfiguredModel::Processor(ProcessorConfig::ComicOnomatopoeia(config)) => Box::new(
                comic_onomatopoeia::ComicOnomatopoeiaProcessor::load(device, config).await?,
            ),
            ConfiguredModel::Processor(ProcessorConfig::MaskFusion(config)) => {
                Box::new(mask_fusion::MaskFusionProcessor::new(config))
            }
            ConfiguredModel::Processor(ProcessorConfig::MangaOcr(config)) => {
                Box::new(manga_ocr::MangaOcrProcessor::load(device, config).await?)
            }
            ConfiguredModel::Processor(ProcessorConfig::BaberuOcr(config)) => {
                Box::new(baberu_ocr::BaberuOcrProcessor::load(device, config).await?)
            }
            ConfiguredModel::Processor(ProcessorConfig::PaddleOcrVl1_6(config)) => {
                Box::new(paddle_ocr_vl_1_6::PaddleOcrVl1_6Processor::load(device, config).await?)
            }
            ConfiguredModel::Translation(config) => {
                Box::new(translation::TranslationProcessor::load(device, config).await?)
            }
            ConfiguredModel::Processor(ProcessorConfig::FontDetector(config)) => {
                Box::new(font_detector::FontDetectorProcessor::load(device, config).await?)
            }
            ConfiguredModel::Processor(ProcessorConfig::LaMa(config)) => {
                Box::new(lama::LaMaProcessor::load(device, config).await?)
            }
            ConfiguredModel::Processor(ProcessorConfig::AotInpainting(config)) => {
                Box::new(aot_inpainting::AotInpaintingProcessor::load(device, config).await?)
            }
            ConfiguredModel::Processor(ProcessorConfig::Flux2Klein(config)) => {
                Box::new(flux2_klein::Flux2KleinProcessor::load(device, config).await?)
            }
            ConfiguredModel::Processor(ProcessorConfig::RoremMixed(config)) => {
                Box::new(rorem_mixed::RoremMixedProcessor::load(device, config).await?)
            }
        })
    }
}
