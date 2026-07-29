mod detection;
mod inpainting;
mod ocr;
mod translation;
mod typography;

use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use koharu_scene::{Generation, ProducerId, SceneEdit, ScenePatch};

pub use detection::KoharuLayoutRFDetrSeg2XLConfig;
pub use inpainting::{
    AotInpaintingConfig, Flux2KleinConfig, LaMaConfig, LaMaHDStrategy, RoremMixedConfig,
};
pub use ocr::{BaberuOcrConfig, MangaOcrConfig, PaddleOcrVl1_6Config};

use crate::{ConfiguredNode, LoadContext, NodeInput, NodeOutput, Processor, ProcessorSpec, Stage};

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
        loaded: tokio::sync::Mutex::new(None),
    }))
}

struct BuiltinProcessor {
    spec: ProcessorSpec,
    node: ConfiguredNode,
    device: koharu_ml::Device,
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
