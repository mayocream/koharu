use std::{collections::BTreeMap, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;

use crate::{CancellationToken, NodeMeasurements, NormalizedScope, RunId, Stage, cache::RunCache};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessorSpec {
    pub stage: Stage,
    pub model: String,
    pub local: bool,
}

#[derive(Clone)]
pub(crate) struct LoadContext {
    pub cancellation: CancellationToken,
}

#[derive(Clone, Debug)]
pub(crate) struct RunOptions {
    pub target_language: String,
    pub translation_instructions: Option<String>,
}

#[derive(Clone)]
pub(crate) struct NodeInput {
    #[allow(dead_code)] // Reserved for deterministic per-run provenance in processors.
    pub run: RunId,
    pub scene: koharu_scene::SceneSnapshot,
    pub scope: Arc<NormalizedScope>,
    pub options: Arc<RunOptions>,
    pub cache: Arc<RunCache>,
    #[allow(dead_code)] // Optional processor-to-descendant data that is not persisted.
    pub artifacts: AncestorArtifacts,
    pub cancellation: CancellationToken,
}

pub(crate) type ArtifactId = Arc<str>;
pub(crate) type Artifact = Arc<dyn std::any::Any + Send + Sync>;
pub(crate) type ArtifactSet = BTreeMap<ArtifactId, Artifact>;
pub(crate) type AncestorArtifacts = Arc<BTreeMap<Stage, ArtifactSet>>;

pub(crate) struct NodeOutput {
    pub patch: koharu_scene::ScenePatch,
    pub artifacts: ArtifactSet,
    pub measurements: NodeMeasurements,
}

#[async_trait]
pub(crate) trait Processor: Send + Sync {
    fn spec(&self) -> &ProcessorSpec;

    async fn ensure_loaded(&self, context: &LoadContext) -> Result<()>;

    async fn run(&self, input: NodeInput) -> Result<NodeOutput>;

    fn try_unload(&self) -> Result<bool> {
        Ok(false)
    }
}
