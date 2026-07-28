mod detection;
mod inpainting;
mod ocr;
mod translation;
mod typography;

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use async_trait::async_trait;
use koharu_scene::{Generation, ProducerId, SceneEdit, ScenePatch};

pub use detection::KoharuLayoutRFDetrSeg2XLConfig;
pub use inpainting::{
    AotInpaintingConfig, Flux2KleinConfig, LaMaConfig, LaMaHDStrategy, RoremMixedConfig,
};
pub use ocr::{BaberuOcrConfig, MangaOcrConfig, PaddleOcrVl1_6Config};

use crate::{
    ConfiguredNode, DownloadContext, LoadContext, NodeInput, NodeOutput, Processor, ProcessorSpec,
    Stage,
};

pub(crate) fn build(
    node: &ConfiguredNode,
    device: koharu_ml::Device,
) -> Result<Arc<dyn Processor>> {
    Ok(Arc::new(BuiltinProcessor {
        spec: ProcessorSpec {
            stage: node.stage(),
            model: node.model(),
            local: node.local(),
        },
        node: node.clone(),
        device,
        downloaded: tokio::sync::OnceCell::new(),
        loaded: tokio::sync::Mutex::new(None),
    }))
}

struct BuiltinProcessor {
    spec: ProcessorSpec,
    node: ConfiguredNode,
    device: koharu_ml::Device,
    downloaded: tokio::sync::OnceCell<()>,
    loaded: tokio::sync::Mutex<Option<Loaded>>,
}

enum Loaded {
    Detection(DetectionModels),
    Ocr(ocr::Model),
    Translation(translation::Model),
    Inpainting(inpainting::Model),
}

struct DetectionModels {
    layout: detection::Model,
    typography: typography::Model,
}

impl DetectionModels {
    async fn load(device: koharu_ml::Device, config: &crate::DetectionModel) -> Result<Self> {
        let (layout, typography) = tokio::try_join!(
            detection::Model::load(device.clone(), config),
            typography::Model::load(device),
        )?;
        Ok(Self { layout, typography })
    }

    async fn run(&self, input: NodeInput) -> Result<NodeOutput> {
        let detected = self.layout.run(input.clone()).await?;
        let preview = input.scene.preview([&detected.patch])?;
        let styled = self
            .typography
            .run(NodeInput {
                scene: preview,
                ..input
            })
            .await?;
        let patch = ScenePatch::merge([&detected.patch, &styled.patch])?;
        let mut artifacts = detected.artifacts;
        for (id, artifact) in styled.artifacts {
            if artifacts.insert(id.clone(), artifact).is_some() {
                bail!("detection sub-models produced duplicate artifact {id}");
            }
        }
        let mut measurements = detected.measurements;
        measurements.queue += styled.measurements.queue;
        measurements.load += styled.measurements.load;
        measurements.execution += styled.measurements.execution;
        Ok(NodeOutput {
            patch,
            artifacts,
            measurements,
        })
    }
}

impl Loaded {
    async fn load(node: &ConfiguredNode, device: koharu_ml::Device) -> Result<Self> {
        match node {
            ConfiguredNode::Detection(config) => DetectionModels::load(device, config)
                .await
                .map(Self::Detection),
            ConfiguredNode::Ocr(config) => ocr::Model::load(device, config).await.map(Self::Ocr),
            ConfiguredNode::Translation(config) => translation::Model::load(device, config)
                .await
                .map(Self::Translation),
            ConfiguredNode::Inpainting(config) => inpainting::Model::load(device, config)
                .await
                .map(Self::Inpainting),
        }
    }

    async fn run(&self, input: NodeInput) -> Result<NodeOutput> {
        match self {
            Self::Detection(model) => model.run(input).await,
            Self::Ocr(model) => model.run(input).await,
            Self::Translation(model) => model.run(input).await,
            Self::Inpainting(model) => model.run(input).await,
        }
    }
}

#[async_trait]
impl Processor for BuiltinProcessor {
    fn spec(&self) -> &ProcessorSpec {
        &self.spec
    }

    fn is_downloaded(&self) -> bool {
        is_downloaded(&self.node)
    }

    async fn ensure_downloaded(&self, context: &DownloadContext) -> Result<()> {
        if context.cancellation.is_cancelled() {
            bail!("model download was cancelled");
        }
        self.downloaded
            .get_or_try_init(|| download(&self.node))
            .await
            .map(|_| ())
    }

    async fn ensure_loaded(&self, context: &LoadContext) -> Result<()> {
        if context.cancellation.is_cancelled() {
            bail!("model load was cancelled");
        }
        let mut loaded = self.loaded.lock().await;
        if loaded.is_none() {
            *loaded = Some(Loaded::load(&self.node, self.device.clone()).await?);
        }
        Ok(())
    }

    async fn run(&self, input: NodeInput) -> Result<NodeOutput> {
        if input.cancellation.is_cancelled() {
            bail!("stage was cancelled");
        }
        let loaded = self.loaded.lock().await;
        loaded
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("model was not loaded"))?
            .run(input)
            .await
    }

    fn try_unload(&self) -> Result<bool> {
        let Ok(mut loaded) = self.loaded.try_lock() else {
            return Ok(false);
        };
        Ok(loaded.take().is_some())
    }
}

async fn download(node: &ConfiguredNode) -> Result<()> {
    match node {
        ConfiguredNode::Detection(_) => tokio::try_join!(
            koharu_ml::koharu_layout_rfdetr_seg_2xl::KoharuLayoutRFDetrSeg2XL::download(),
            koharu_ml::font_detector::FontDetector::download(),
        )
        .map(|_| ()),
        ConfiguredNode::Ocr(crate::OcrModel::MangaOcr(_)) => {
            koharu_ml::manga_ocr::MangaOcr::download().await
        }
        ConfiguredNode::Ocr(crate::OcrModel::BaberuOcr(_)) => {
            koharu_ml::baberu_ocr::BaberuOcr::download().await
        }
        ConfiguredNode::Ocr(crate::OcrModel::PaddleOcrVl1_6(_)) => {
            koharu_ml::paddle_ocr_vl::PaddleOCRVL::download().await
        }
        ConfiguredNode::Translation(koharu_translator::Providers::Local(config)) => {
            let model = config
                .model
                .parse::<koharu_translator::LocalModel>()
                .with_context(|| format!("unknown local translator '{}'", config.model))?;
            koharu_translator::LocalTranslator::download(model)
                .await
                .map_err(Into::into)
        }
        ConfiguredNode::Translation(_) => Ok(()),
        ConfiguredNode::Inpainting(crate::InpaintingModel::LaMa(_)) => {
            koharu_ml::lama::LaMa::download().await
        }
        ConfiguredNode::Inpainting(crate::InpaintingModel::AotInpainting(_)) => {
            koharu_ml::aot_inpainting::AotInpainting::download().await
        }
        ConfiguredNode::Inpainting(crate::InpaintingModel::Flux2Klein(_)) => {
            koharu_ml::flux2_klein::Flux2KleinInpaint::download().await
        }
        ConfiguredNode::Inpainting(crate::InpaintingModel::RoremMixed(_)) => {
            koharu_ml::rorem_mixed::RoremMixed::download().await
        }
    }
}

fn is_downloaded(node: &ConfiguredNode) -> bool {
    match node {
        ConfiguredNode::Detection(_) => {
            koharu_ml::koharu_layout_rfdetr_seg_2xl::KoharuLayoutRFDetrSeg2XL::is_downloaded()
                && koharu_ml::font_detector::FontDetector::is_downloaded()
        }
        ConfiguredNode::Ocr(crate::OcrModel::MangaOcr(_)) => {
            koharu_ml::manga_ocr::MangaOcr::is_downloaded()
        }
        ConfiguredNode::Ocr(crate::OcrModel::BaberuOcr(_)) => {
            koharu_ml::baberu_ocr::BaberuOcr::is_downloaded()
        }
        ConfiguredNode::Ocr(crate::OcrModel::PaddleOcrVl1_6(_)) => {
            koharu_ml::paddle_ocr_vl::PaddleOCRVL::is_downloaded()
        }
        ConfiguredNode::Translation(koharu_translator::Providers::Local(config)) => config
            .model
            .parse::<koharu_translator::LocalModel>()
            .is_ok_and(koharu_translator::LocalTranslator::is_downloaded),
        ConfiguredNode::Translation(_) => true,
        ConfiguredNode::Inpainting(crate::InpaintingModel::LaMa(_)) => {
            koharu_ml::lama::LaMa::is_downloaded()
        }
        ConfiguredNode::Inpainting(crate::InpaintingModel::AotInpainting(_)) => {
            koharu_ml::aot_inpainting::AotInpainting::is_downloaded()
        }
        ConfiguredNode::Inpainting(crate::InpaintingModel::Flux2Klein(_)) => {
            koharu_ml::flux2_klein::Flux2KleinInpaint::is_downloaded()
        }
        ConfiguredNode::Inpainting(crate::InpaintingModel::RoremMixed(_)) => {
            koharu_ml::rorem_mixed::RoremMixed::is_downloaded()
        }
    }
}

fn generation(producer: &str, model: &str) -> Result<Generation> {
    let mut generation = Generation::new(ProducerId::new(producer)?);
    generation.model = Some(model.to_owned());
    Ok(generation)
}

fn finish(edit: SceneEdit) -> Result<NodeOutput> {
    Ok(NodeOutput {
        patch: edit.finish()?,
        artifacts: Default::default(),
        measurements: Default::default(),
    })
}

fn producer(stage: Stage) -> &'static str {
    match stage {
        Stage::Detection => "dev.koharu.pipeline.detection",
        Stage::Ocr => "dev.koharu.pipeline.ocr",
        Stage::Translation => "dev.koharu.pipeline.translation",
        Stage::Inpainting => "dev.koharu.pipeline.inpainting",
    }
}
