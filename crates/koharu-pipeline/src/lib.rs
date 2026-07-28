//! In-process, scene-native model orchestration for Koharu.

mod builtin;
mod cache;
mod config;
mod events;
mod graph;
mod node;
mod processor;
mod resources;
mod run;
mod scheduler;
mod scope;
mod status;

use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr as _,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result, bail};
use arc_swap::ArcSwap;
use koharu_scene::{Asset, Geometry, SceneComponent, SceneSnapshot, SourceText};

pub use builtin::{
    AotInpaintingConfig, BaberuOcrConfig, Flux2KleinConfig, KoharuLayoutRFDetrSeg2XLConfig,
    LaMaConfig, LaMaHDStrategy, MangaOcrConfig, PaddleOcrVl1_6Config, RoremMixedConfig,
};
pub use config::{DetectionModel, InpaintingModel, OcrModel, PipelineConfig};
pub use events::{CancellationToken, EventSink, PipelineEvent, RunId, UnloadReason};
pub use graph::{Dependency, Stage, Target};
pub use run::{NodeMeasurements, NodeReport, Run, RunError, RunReport};
pub use scope::{Bounds, Scope};
pub use status::{
    ConfigRevision, DeviceResources, DownloadState, LoadState, LoadedModelResources, ModelStatus,
    ResourceSnapshot,
};

use cache::RunCache;
use events::EventHub;
use graph::{PipelineGraph, Selection};
use node::ConfiguredNode;
use processor::{
    AncestorArtifacts, DownloadContext, LoadContext, NodeInput, NodeOutput, Processor,
    ProcessorSpec, RunOptions,
};
use resources::ResourceMonitor;
use scope::NormalizedScope;
use status::ModelStatusHub;

pub(crate) struct ConfigurationGeneration {
    revision: ConfigRevision,
    pipeline: Arc<PipelineConfig>,
    translation: Arc<koharu_translator::TranslationConfig>,
    nodes: BTreeMap<Stage, ConfiguredNode>,
    processors: BTreeMap<Stage, Arc<dyn Processor>>,
    usage: BTreeMap<Stage, Arc<tokio::sync::Mutex<()>>>,
}

#[derive(Clone, Debug)]
pub struct ConfigChange {
    pub revision: ConfigRevision,
    pub changed: Vec<Stage>,
}

#[derive(Clone, Debug, Default)]
pub struct DownloadReport {
    pub downloaded: Vec<Stage>,
}

pub struct Pipeline {
    current: ArcSwap<ConfigurationGeneration>,
    reconfiguration: std::sync::Mutex<()>,
    next_revision: AtomicU64,
    graph: PipelineGraph,
    device: koharu_ml::Device,
    model_status: Arc<ModelStatusHub>,
    events: Arc<EventHub>,
    resources: Arc<ResourceMonitor>,
}

impl Pipeline {
    pub fn new(
        config: PipelineConfig,
        translation: koharu_translator::TranslationConfig,
    ) -> Result<Self> {
        validate_configuration(&config, &translation)?;
        let graph = PipelineGraph::new()?;
        let device = koharu_ml::device(false);
        let nodes = configured_nodes(&config, &translation);
        let processors = build_processors(&nodes, &device, None)?;
        let usage = build_usage(&nodes, None);
        let revision = ConfigRevision(1);
        let generation = Arc::new(ConfigurationGeneration {
            revision,
            pipeline: Arc::new(config),
            translation: Arc::new(translation),
            nodes,
            processors,
            usage,
        });
        let model_status = Arc::new(ModelStatusHub::new());
        model_status.install(revision, status_models(&generation, None));
        inspect_downloads(&model_status, &generation);
        let resources = ResourceMonitor::new(&device, model_status.clone());
        Ok(Self {
            current: ArcSwap::new(generation),
            reconfiguration: std::sync::Mutex::new(()),
            next_revision: AtomicU64::new(2),
            graph,
            device,
            model_status,
            events: Arc::new(EventHub::new()),
            resources,
        })
    }

    pub fn reconfigure(
        &self,
        config: PipelineConfig,
        translation: koharu_translator::TranslationConfig,
    ) -> Result<ConfigChange> {
        validate_configuration(&config, &translation)?;
        let _reconfiguration = self
            .reconfiguration
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous = self.current.load_full();
        let nodes = configured_nodes(&config, &translation);
        let changed = Stage::ALL
            .into_iter()
            .filter(|stage| previous.nodes.get(stage) != nodes.get(stage))
            .collect::<Vec<_>>();
        if changed.is_empty()
            && previous.translation.target_language == translation.target_language
            && previous.translation.instructions == translation.instructions
        {
            return Ok(ConfigChange {
                revision: previous.revision,
                changed,
            });
        }
        let revision = ConfigRevision(self.next_revision.fetch_add(1, Ordering::Relaxed));
        let processors = build_processors(&nodes, &self.device, Some(&previous))?;
        let usage = build_usage(&nodes, Some(&previous));
        let generation = Arc::new(ConfigurationGeneration {
            revision,
            pipeline: Arc::new(config),
            translation: Arc::new(translation),
            nodes,
            processors,
            usage,
        });
        self.model_status
            .install(revision, status_models(&generation, Some(&previous)));
        inspect_downloads(&self.model_status, &generation);
        self.current.store(generation);
        self.events.emit(PipelineEvent::ConfigurationChanged {
            generation: revision,
            changed: changed.clone(),
        });
        Ok(ConfigChange { revision, changed })
    }

    #[must_use]
    pub fn configuration(
        &self,
    ) -> (
        ConfigRevision,
        Arc<PipelineConfig>,
        Arc<koharu_translator::TranslationConfig>,
    ) {
        let generation = self.current.load_full();
        (
            generation.revision,
            generation.pipeline.clone(),
            generation.translation.clone(),
        )
    }

    pub fn graph(&self) -> String {
        self.graph.dot()
    }

    #[must_use]
    pub fn run(&self, snapshot: SceneSnapshot) -> Run<'_> {
        Run {
            pipeline: self,
            snapshot,
            request: Default::default(),
        }
    }

    #[must_use]
    pub fn model_status(&self) -> Arc<[ModelStatus]> {
        self.model_status.snapshot()
    }

    pub fn subscribe_model_status(&self) -> tokio::sync::watch::Receiver<Arc<[ModelStatus]>> {
        self.model_status.subscribe()
    }

    pub fn subscribe_resources(&self) -> tokio::sync::watch::Receiver<ResourceSnapshot> {
        self.resources.start();
        self.resources.subscribe()
    }

    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<PipelineEvent> {
        self.events.subscribe()
    }

    pub async fn download_models(
        &self,
        stages: impl IntoIterator<Item = Stage>,
    ) -> Result<DownloadReport> {
        let generation = self.current.load_full();
        let stages = stages.into_iter().collect::<BTreeSet<_>>();
        if stages.is_empty() {
            bail!("no models were selected for download");
        }
        self.download_selected(&generation, &stages, CancellationToken::default(), None)
            .await
            .map_err(|(stage, error)| error.context(format!("failed to download {stage} model")))?;
        let downloaded = self
            .graph
            .canonical()
            .iter()
            .filter(|stage| stages.contains(stage))
            .copied()
            .collect();
        Ok(DownloadReport { downloaded })
    }

    fn preflight(
        &self,
        snapshot: &SceneSnapshot,
        selection: &Selection,
        scope: &NormalizedScope,
    ) -> Result<()> {
        for page in scope.pages() {
            if selection.stages.contains(&Stage::Detection)
                && snapshot.component::<Asset>(*page, "source")?.is_none()
            {
                bail!("page {page} has no source asset");
            }
        }
        if !selection.exact {
            return Ok(());
        }
        if selection.stages.contains(&Stage::Translation)
            && !selection.stages.contains(&Stage::Ocr)
            && !scope_has::<SourceText>(snapshot, scope, "default")?
        {
            bail!("exact translation requires existing source text");
        }
        if selection.stages.contains(&Stage::Ocr)
            && !selection.stages.contains(&Stage::Detection)
            && !scope_has::<Geometry>(snapshot, scope, "default")?
        {
            bail!("exact OCR requires existing detected geometry");
        }
        if selection.stages.contains(&Stage::Inpainting)
            && !selection.stages.contains(&Stage::Detection)
            && !scope.pages().iter().any(|page| {
                snapshot
                    .component::<Asset>(*page, "text-mask")
                    .ok()
                    .flatten()
                    .is_some()
            })
        {
            bail!("exact inpainting requires an existing text-mask asset");
        }
        Ok(())
    }
}

fn scope_has<T: SceneComponent>(
    snapshot: &SceneSnapshot,
    scope: &NormalizedScope,
    slot: &str,
) -> Result<bool> {
    for entity in snapshot.entities_with::<T>(slot)? {
        if scope.contains_entity(snapshot, entity.id())? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_configuration(
    config: &PipelineConfig,
    translation: &koharu_translator::TranslationConfig,
) -> Result<()> {
    config.validate()?;
    koharu_translator::Language::from_str(&translation.target_language).with_context(|| {
        format!(
            "unsupported target language {}",
            translation.target_language
        )
    })?;
    if let koharu_translator::Providers::Local(config) = &translation.model {
        koharu_translator::LocalModel::from_str(&config.model)
            .with_context(|| format!("unknown local translator '{}'", config.model))?;
    }
    if translation
        .instructions
        .as_ref()
        .is_some_and(|value| value.contains('\0') || value.len() > 1024 * 1024)
    {
        bail!("translation instructions are too large or contain NUL");
    }
    Ok(())
}

fn configured_nodes(
    config: &PipelineConfig,
    translation: &koharu_translator::TranslationConfig,
) -> BTreeMap<Stage, ConfiguredNode> {
    config
        .nodes(translation)
        .into_iter()
        .map(|node| (node.stage(), node))
        .collect()
}

fn build_processors(
    nodes: &BTreeMap<Stage, ConfiguredNode>,
    device: &koharu_ml::Device,
    previous: Option<&ConfigurationGeneration>,
) -> Result<BTreeMap<Stage, Arc<dyn Processor>>> {
    nodes
        .iter()
        .map(|(stage, node)| {
            let processor = previous
                .filter(|generation| generation.nodes.get(stage) == Some(node))
                .and_then(|generation| generation.processors.get(stage).cloned())
                .map_or_else(|| builtin::build(node, device.clone()), Ok)?;
            Ok((*stage, processor))
        })
        .collect()
}

fn build_usage(
    nodes: &BTreeMap<Stage, ConfiguredNode>,
    previous: Option<&ConfigurationGeneration>,
) -> BTreeMap<Stage, Arc<tokio::sync::Mutex<()>>> {
    nodes
        .iter()
        .map(|(stage, node)| {
            let usage = previous
                .filter(|generation| generation.nodes.get(stage) == Some(node))
                .and_then(|generation| generation.usage.get(stage).cloned())
                .unwrap_or_else(|| Arc::new(tokio::sync::Mutex::new(())));
            (*stage, usage)
        })
        .collect()
}

fn status_models<'a>(
    generation: &'a ConfigurationGeneration,
    previous: Option<&'a ConfigurationGeneration>,
) -> impl Iterator<Item = (Stage, String, bool, bool)> + 'a {
    generation.nodes.values().map(move |node| {
        let stage = node.stage();
        (
            stage,
            node.model(),
            node.local(),
            previous.is_some_and(|generation| generation.nodes.get(&stage) == Some(node)),
        )
    })
}

fn inspect_downloads(status: &ModelStatusHub, generation: &ConfigurationGeneration) {
    for (stage, processor) in &generation.processors {
        if processor.spec().local {
            status.download(
                generation.revision,
                *stage,
                if processor.is_downloaded() {
                    DownloadState::Downloaded
                } else {
                    DownloadState::Missing
                },
            );
        }
    }
}

#[cfg(test)]
mod tests;
