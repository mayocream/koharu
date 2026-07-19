//! Typed, scene-native model orchestration for Koharu.

mod builtin;
mod config;
mod context;
mod events;
mod plan;
mod run;
mod worker;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::join_all;
use koharu_config::Config;
use koharu_ml::Device;
use koharu_scene::{
    BlobId, Command, Commands, ElementChange, ElementId, ElementKind, Frame, Page, PageAsset,
    PageId, ProjectId, RegionKind, Revision, Session, TextRole,
};
use koharu_translator::TranslationConfig;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Mutex as AsyncMutex;

pub use builtin::{
    AotInpaintingConfig, BaberuOcrConfig, ComicLayoutYolo26sConfig, ComicOnomatopoeiaConfig,
    ComicTextDetectorConfig, Flux2KleinConfig, FontDetectorConfig, LaMaConfig, MangaOcrConfig,
    MangaTextMaskConfig, MaskFusionConfig, PPDocLayoutV3Config, PaddleOcrVl1_6Config,
    RoremMixedConfig, Yolo11nSpeechBubbleConfig, YoloV8mSpeechBubbleConfig,
};
pub use config::*;
pub use context::{BlobBytes, Context};
pub use events::*;
pub use run::{Force, Run, RunError, RunReport, RunTarget};
pub use worker::serve_worker;

use plan::{ConfiguredModel, Plan, PlanNode};
use run::RunRequest;
use worker::WorkerFactory;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Detection,
    Segmentation,
    Ocr,
    Translation,
    Typography,
    Inpainting,
}

impl fmt::Display for Phase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Detection => "detection",
            Self::Segmentation => "segmentation",
            Self::Ocr => "ocr",
            Self::Translation => "translation",
            Self::Typography => "typography",
            Self::Inpainting => "inpainting",
        })
    }
}

impl Phase {
    pub const ALL: [Self; 6] = [
        Self::Detection,
        Self::Segmentation,
        Self::Ocr,
        Self::Translation,
        Self::Typography,
        Self::Inpainting,
    ];
}

impl FromStr for Phase {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "detection" | "detect" => Ok(Self::Detection),
            "segmentation" | "segment" => Ok(Self::Segmentation),
            "ocr" => Ok(Self::Ocr),
            "translation" | "translate" => Ok(Self::Translation),
            "typography" | "type" => Ok(Self::Typography),
            "inpainting" | "inpaint" => Ok(Self::Inpainting),
            _ => bail!("unknown pipeline phase '{value}'"),
        }
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Artifact {
    SourceImage,
    PanelRegion,
    BubbleRegion,
    TextRegion,
    CooRegion,
    TextMaskCandidate,
    LayoutTextMask,
    TextMask,
    CooMask,
    BrushMask,
    BubbleMask,
    SourceText,
    CooText,
    Translation,
    Typography,
    CleanImage,
}

impl fmt::Display for Artifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SourceImage => "source image",
            Self::PanelRegion => "panel region",
            Self::BubbleRegion => "bubble region",
            Self::TextRegion => "text region",
            Self::CooRegion => "COO region",
            Self::TextMaskCandidate => "text mask candidate",
            Self::LayoutTextMask => "layout text mask",
            Self::TextMask => "text mask",
            Self::CooMask => "COO mask",
            Self::BrushMask => "brush mask",
            Self::BubbleMask => "bubble mask",
            Self::SourceText => "source text",
            Self::CooText => "COO text",
            Self::Translation => "translation",
            Self::Typography => "typography",
            Self::CleanImage => "clean image",
        })
    }
}

impl Artifact {
    const fn phase(self) -> Option<Phase> {
        match self {
            Self::SourceImage | Self::BrushMask => None,
            Self::PanelRegion | Self::BubbleRegion | Self::TextRegion | Self::CooRegion => {
                Some(Phase::Detection)
            }
            Self::TextMaskCandidate
            | Self::LayoutTextMask
            | Self::TextMask
            | Self::CooMask
            | Self::BubbleMask => Some(Phase::Segmentation),
            Self::SourceText | Self::CooText => Some(Phase::Ocr),
            Self::Translation => Some(Phase::Translation),
            Self::Typography => Some(Phase::Typography),
            Self::CleanImage => Some(Phase::Inpainting),
        }
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorId {
    ComicTextDetector,
    #[serde(rename = "pp_doclayout_v3")]
    PPDocLayoutV3,
    ComicLayoutYolo26s,
    MangaTextMask,
    #[serde(rename = "speech_bubble_yolov8m")]
    SpeechBubbleYoloV8m,
    #[serde(rename = "speech_bubble_yolo11n")]
    SpeechBubbleYolo11n,
    ComicOnomatopoeia,
    MaskFusion,
    #[serde(rename = "paddleocr_vl_1.6")]
    PaddleOcrVl1_6,
    MangaOcr,
    BaberuOcr,
    Translation,
    FontDetector,
    #[serde(rename = "lama")]
    LaMa,
    AotInpainting,
    Flux2Klein,
    RoremMixed,
}

impl fmt::Display for ProcessorId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ComicTextDetector => "comic_text_detector",
            Self::PPDocLayoutV3 => "pp_doclayout_v3",
            Self::ComicLayoutYolo26s => "comic_layout_yolo26s",
            Self::MangaTextMask => "manga_text_mask",
            Self::SpeechBubbleYoloV8m => "speech_bubble_yolov8m",
            Self::SpeechBubbleYolo11n => "speech_bubble_yolo11n",
            Self::ComicOnomatopoeia => "comic_onomatopoeia",
            Self::MaskFusion => "mask_fusion",
            Self::PaddleOcrVl1_6 => "paddleocr_vl_1.6",
            Self::MangaOcr => "manga_ocr",
            Self::BaberuOcr => "baberu_ocr",
            Self::Translation => "translation",
            Self::FontDetector => "font_detector",
            Self::LaMa => "lama",
            Self::AotInpainting => "aot_inpainting",
            Self::Flux2Klein => "flux2_klein",
            Self::RoremMixed => "rorem_mixed",
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Project,
    Pages {
        pages: Vec<PageId>,
    },
    Region {
        page: PageId,
        frame: Frame,
    },
    Elements {
        elements: Vec<ElementId>,
    },
}

#[async_trait]
pub trait Processor: Send {
    fn name(&self) -> &'static str;
    fn inputs(&self) -> &'static [Artifact];
    fn outputs(&self) -> &'static [Artifact];
    async fn shutdown(&mut self) {}
    async fn run(&mut self, context: &Context) -> Result<Commands>;
}

#[async_trait]
trait ProcessorFactory: Send + Sync {
    async fn create(&self, model: &ConfiguredModel, device: Device) -> Result<Box<dyn Processor>>;
}

struct ProcessorEntry {
    model: ConfiguredModel,
    processor: Arc<AsyncMutex<Box<dyn Processor>>>,
}

#[derive(Clone, Eq, Hash, PartialEq)]
struct FreshKey {
    project: ProjectId,
    processor: ProcessorId,
    scope: Vec<u8>,
}

#[derive(Clone, Eq, PartialEq)]
struct FreshRecord {
    model: [u8; 32],
    inputs: [u8; 32],
    outputs: BTreeMap<Artifact, [u8; 32]>,
}

pub struct Pipeline {
    config: Config<PipelineConfig>,
    translation: Config<TranslationConfig>,
    device: Device,
    factory: Arc<dyn ProcessorFactory>,
    processors: AsyncMutex<BTreeMap<ProcessorId, ProcessorEntry>>,
    freshness: Mutex<HashMap<FreshKey, FreshRecord>>,
    accelerator: AsyncMutex<()>,
    run_lock: AsyncMutex<()>,
}

impl Pipeline {
    #[must_use]
    pub fn new(
        config: impl Into<Config<PipelineConfig>>,
        translation: impl Into<Config<TranslationConfig>>,
    ) -> Self {
        Self::with_factory(
            config.into(),
            translation.into(),
            Arc::new(WorkerFactory::default()),
        )
    }

    #[must_use]
    pub fn with_worker_executable(
        config: impl Into<Config<PipelineConfig>>,
        translation: impl Into<Config<TranslationConfig>>,
        executable: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self::with_factory(
            config.into(),
            translation.into(),
            Arc::new(WorkerFactory::with_executable(executable.into())),
        )
    }

    fn with_factory(
        config: Config<PipelineConfig>,
        translation: Config<TranslationConfig>,
        factory: Arc<dyn ProcessorFactory>,
    ) -> Self {
        Self {
            config,
            translation,
            device: koharu_ml::device(false),
            factory,
            processors: AsyncMutex::new(BTreeMap::new()),
            freshness: Mutex::new(HashMap::new()),
            accelerator: AsyncMutex::new(()),
            run_lock: AsyncMutex::new(()),
        }
    }

    pub fn check(&self) -> Result<()> {
        let config = self.config.read()?.clone();
        let translation = self.translation.read()?.clone();
        Plan::build(&config, &translation.model).map(|_| ())
    }

    pub fn graph(&self) -> Result<String> {
        let config = self.config.read()?.clone();
        let translation = self.translation.read()?.clone();
        Ok(Plan::build(&config, &translation.model)?.dot())
    }

    #[must_use]
    pub fn run<'pipeline, 'session>(
        &'pipeline self,
        session: &'session mut Session,
    ) -> Run<'pipeline, 'session> {
        Run {
            pipeline: self,
            session,
            request: RunRequest::default(),
        }
    }

    pub async fn unload_all(&self) -> Result<()> {
        let _run = self.run_lock.lock().await;
        let removed = std::mem::take(&mut *self.processors.lock().await);
        Self::shutdown_loaded(removed.into_values()).await;
        Ok(())
    }

    async fn execute(
        &self,
        session: &mut Session,
        request: RunRequest,
    ) -> std::result::Result<RunReport, RunError> {
        let mut revisions = Vec::new();
        let measurements = request.context.measurements.clone();
        let result = self.execute_inner(session, request, &mut revisions).await;
        let measurements = measurements
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        match result {
            Ok((processors, skipped)) => Ok(RunReport {
                revisions,
                processors,
                skipped,
                measurements,
            }),
            Err(source) => Err(RunError {
                source,
                committed_revisions: revisions,
                measurements,
            }),
        }
    }

    async fn execute_inner(
        &self,
        session: &mut Session,
        mut request: RunRequest,
        revisions: &mut Vec<Revision>,
    ) -> Result<(usize, usize)> {
        let _run = self.run_lock.lock().await;
        let config = self.config.read()?.clone();
        let translation = self.translation.read()?.clone();
        request.context.translation = context::TranslationOptions {
            target_language: translation.target_language,
            instructions: translation.instructions,
        };
        let all = Plan::build(&config, &translation.model)?;
        self.reconcile(&all).await;
        let selected = all.select(&request.target)?;
        let selected_count = selected.nodes.len();
        let execution = self.execution_mask(session, &request, &selected)?;
        let plan = selected.retain(&execution)?;
        let skipped = selected_count - plan.nodes.len();
        let mut blobs = HashMap::new();
        let decoded = Arc::new(Mutex::new(HashMap::new()));
        let completed = AtomicUsize::new(0);

        for wave in &plan.waves {
            if request.context.cancellation.is_cancelled() {
                bail!("pipeline run was cancelled");
            }
            let context = Arc::new(capture(session, &request, &mut blobs, decoded.clone())?);
            context.validate_scope()?;
            let nodes = wave
                .iter()
                .map(|&index| &plan.nodes[index])
                .collect::<Vec<_>>();
            let input_fingerprints = nodes
                .iter()
                .map(|node| artifact_fingerprint(session, &request.scope, node.model.inputs()))
                .collect::<Result<Vec<_>>>()?;
            for node in &nodes {
                validate_inputs(&node.model, &context)?;
            }
            let processors = self.ensure_processors(&nodes).await?;
            let futures = nodes.iter().zip(processors).map(|(node, processor)| {
                let context = Arc::new(context.for_phase(node.phase));
                async move {
                    if context.cancellation().is_cancelled() {
                        bail!("pipeline run was cancelled");
                    }
                    let _accelerator = if node.model.uses_accelerator()
                        && self.device.backend != koharu_ml::Backend::Cpu
                    {
                        Some(self.accelerator.lock().await)
                    } else {
                        None
                    };
                    let mut processor = processor.lock().await;
                    processor
                        .run(&context)
                        .await
                        .with_context(|| format!("{} failed", node.model.name()))
                }
            });
            let results = join_all(futures).await;
            let mut merged = context.commands();
            for (node, result) in nodes.iter().zip(results) {
                let commands = result?;
                validate_commands(&node.model, &context, &commands)?;
                merged.merge(commands).with_context(|| {
                    format!("{} produced conflicting commands", node.model.name())
                })?;
            }
            if request.context.cancellation.is_cancelled() {
                bail!("pipeline run was cancelled");
            }
            add_invalidations(&context, &mut merged);
            if !merged.as_slice().is_empty() {
                let change = session.apply(merged)?;
                if change.to != change.from {
                    revisions.push(change.to);
                }
            }
            self.record_freshness(
                session,
                &request.scope,
                &request.context.translation,
                nodes.iter().copied().zip(input_fingerprints),
            )?;
            for node in nodes {
                let current = completed.fetch_add(1, Ordering::Relaxed) + 1;
                let update = Progress {
                    phase: node.phase,
                    model: node.model.name().to_owned(),
                    completed: current,
                    total: plan.nodes.len(),
                };
                if let Some(events) = &request.context.events {
                    events(PipelineEvent::Progress(update));
                }
            }
        }
        Ok((plan.nodes.len(), skipped))
    }

    async fn reconcile(&self, plan: &Plan) {
        let wanted = plan
            .nodes
            .iter()
            .map(|node| (node.id, &node.model))
            .collect::<BTreeMap<_, _>>();
        let removed = {
            let mut processors = self.processors.lock().await;
            let keys = processors
                .iter()
                .filter_map(|(key, entry)| {
                    (!wanted.get(key).is_some_and(|model| **model == entry.model)).then_some(*key)
                })
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| processors.remove(&key))
                .collect::<Vec<_>>()
        };
        Self::shutdown_loaded(removed).await;
    }

    async fn shutdown_loaded(processors: impl IntoIterator<Item = ProcessorEntry>) {
        for processor in processors {
            processor.processor.lock().await.shutdown().await;
        }
    }

    async fn ensure_processors(
        &self,
        nodes: &[&PlanNode],
    ) -> Result<Vec<Arc<AsyncMutex<Box<dyn Processor>>>>> {
        let missing = {
            let processors = self.processors.lock().await;
            nodes
                .iter()
                .filter(|node| !processors.contains_key(&node.id))
                .map(|node| (node.id, node.model.clone()))
                .collect::<Vec<_>>()
        };
        let loads = missing.iter().map(|(_, model)| {
            let factory = self.factory.clone();
            let device = self.device.clone();
            async move { factory.create(model, device).await }
        });
        let loaded_processors = join_all(loads).await;
        if !missing.is_empty() {
            let mut processors = self.processors.lock().await;
            for ((key, model), processor) in missing.into_iter().zip(loaded_processors) {
                let processor = processor?;
                validate_processor(&model, processor.as_ref())?;
                processors.insert(
                    key,
                    ProcessorEntry {
                        model,
                        processor: Arc::new(AsyncMutex::new(processor)),
                    },
                );
            }
        }
        let processors = self.processors.lock().await;
        nodes
            .iter()
            .map(|node| {
                processors
                    .get(&node.id)
                    .map(|value| value.processor.clone())
                    .ok_or_else(|| anyhow!("{} was not loaded", node.model.name()))
            })
            .collect()
    }

    fn execution_mask(
        &self,
        session: &Session,
        request: &RunRequest,
        plan: &Plan,
    ) -> Result<Vec<bool>> {
        let scope = serde_json::to_vec(&request.scope)?;
        let project = session.id();
        let current = plan
            .nodes
            .iter()
            .zip(&plan.required)
            .map(|(node, required)| {
                current_record(
                    session,
                    &request.scope,
                    &node.model,
                    required,
                    &request.context.translation,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let freshness = self
            .freshness
            .lock()
            .map_err(|_| anyhow!("pipeline freshness cache lock is poisoned"))?;
        let mut scheduled = vec![false; plan.nodes.len()];
        for wave in &plan.waves {
            for &index in wave {
                let node = &plan.nodes[index];
                let forced = matches!(request.force, Force::All)
                    || (matches!(request.force, Force::Targets) && plan.targets[index]);
                let key = FreshKey {
                    project,
                    processor: node.id,
                    scope: scope.clone(),
                };
                let fresh = freshness
                    .get(&key)
                    .is_some_and(|record| record.satisfies(&current[index]));
                scheduled[index] = forced || plan.dependency_ran(index, &scheduled) || !fresh;
            }
        }
        Ok(scheduled)
    }

    fn record_freshness<'a>(
        &self,
        session: &Session,
        scope: &Scope,
        translation: &context::TranslationOptions,
        nodes: impl IntoIterator<Item = (&'a PlanNode, [u8; 32])>,
    ) -> Result<()> {
        let scope_bytes = serde_json::to_vec(scope)?;
        let mut records = Vec::new();
        for (node, inputs) in nodes {
            records.push((
                FreshKey {
                    project: session.id(),
                    processor: node.id,
                    scope: scope_bytes.clone(),
                },
                FreshRecord {
                    model: model_fingerprint(&node.model, translation)?,
                    inputs,
                    outputs: output_fingerprints(session, scope, node.model.outputs())?,
                },
            ));
        }
        self.freshness
            .lock()
            .map_err(|_| anyhow!("pipeline freshness cache lock is poisoned"))?
            .extend(records);
        Ok(())
    }
}

impl FreshRecord {
    fn satisfies(&self, current: &Self) -> bool {
        self.model == current.model
            && self.inputs == current.inputs
            && current
                .outputs
                .iter()
                .all(|(artifact, hash)| self.outputs.get(artifact) == Some(hash))
    }
}

fn current_record(
    session: &Session,
    scope: &Scope,
    model: &ConfiguredModel,
    outputs: &[Artifact],
    translation: &context::TranslationOptions,
) -> Result<FreshRecord> {
    Ok(FreshRecord {
        model: model_fingerprint(model, translation)?,
        inputs: artifact_fingerprint(session, scope, model.inputs())?,
        outputs: output_fingerprints(session, scope, outputs)?,
    })
}

fn output_fingerprints(
    session: &Session,
    scope: &Scope,
    outputs: &[Artifact],
) -> Result<BTreeMap<Artifact, [u8; 32]>> {
    outputs
        .iter()
        .map(|output| artifact_fingerprint(session, scope, &[*output]).map(|hash| (*output, hash)))
        .collect()
}

fn model_fingerprint(
    model: &ConfiguredModel,
    translation: &context::TranslationOptions,
) -> Result<[u8; 32]> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(&serde_json::to_vec(model)?);
    if matches!(model, ConfiguredModel::Translation(_)) {
        hasher.update(&serde_json::to_vec(&(
            &translation.target_language,
            &translation.instructions,
        ))?);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn artifact_fingerprint(
    session: &Session,
    scope: &Scope,
    artifacts: &[Artifact],
) -> Result<[u8; 32]> {
    let pages = scoped_pages(session, scope)?;
    let mut hasher = blake3::Hasher::new();
    for artifact in artifacts {
        hasher.update(&serde_json::to_vec(artifact)?);
        for page in &pages {
            hasher.update(&serde_json::to_vec(&page.id)?);
            let bytes = match artifact {
                Artifact::SourceImage => serde_json::to_vec(&(page.source, page.size))?,
                Artifact::PanelRegion => serde_json::to_vec(
                    &page
                        .elements
                        .iter()
                        .filter(|element| {
                            matches!(
                                &element.kind,
                                ElementKind::Region(region) if region.kind == RegionKind::Panel
                            )
                        })
                        .map(|element| (element.id, element.frame))
                        .collect::<Vec<_>>(),
                )?,
                Artifact::BubbleRegion => serde_json::to_vec(
                    &page
                        .elements
                        .iter()
                        .filter(|element| {
                            matches!(
                                &element.kind,
                                ElementKind::Region(region) if region.kind == RegionKind::Bubble
                            )
                        })
                        .map(|element| (element.id, element.frame))
                        .collect::<Vec<_>>(),
                )?,
                Artifact::TextRegion => serde_json::to_vec(
                    &page
                        .elements
                        .iter()
                        .filter(|element| {
                            element
                                .text()
                                .is_some_and(|text| text.role != TextRole::Onomatopoeia)
                        })
                        .map(|element| (element.id, element.frame))
                        .collect::<Vec<_>>(),
                )?,
                Artifact::CooRegion => serde_json::to_vec(
                    &page
                        .elements
                        .iter()
                        .filter_map(|element| {
                            element
                                .text()
                                .filter(|text| text.role == TextRole::Onomatopoeia)
                                .map(|text| (element.id, element.frame, &text.polygon))
                        })
                        .collect::<Vec<_>>(),
                )?,
                Artifact::TextMaskCandidate => {
                    serde_json::to_vec(&page.assets.text_mask_candidate)?
                }
                Artifact::LayoutTextMask => serde_json::to_vec(&page.assets.layout_text_mask)?,
                Artifact::TextMask => serde_json::to_vec(&page.assets.text_mask)?,
                Artifact::CooMask => serde_json::to_vec(&page.assets.coo_mask)?,
                Artifact::BrushMask => serde_json::to_vec(&page.assets.brush_mask)?,
                Artifact::BubbleMask => serde_json::to_vec(&page.assets.bubble_mask)?,
                Artifact::SourceText => serde_json::to_vec(
                    &page
                        .elements
                        .iter()
                        .filter_map(|element| {
                            element
                                .text()
                                .filter(|text| text.role != TextRole::Onomatopoeia)
                                .map(|text| (element.id, text.source.as_ref()))
                        })
                        .collect::<Vec<_>>(),
                )?,
                Artifact::CooText => serde_json::to_vec(
                    &page
                        .elements
                        .iter()
                        .filter_map(|element| {
                            element
                                .text()
                                .filter(|text| text.role == TextRole::Onomatopoeia)
                                .map(|text| (element.id, text.source.as_ref()))
                        })
                        .collect::<Vec<_>>(),
                )?,
                Artifact::Translation => serde_json::to_vec(
                    &page
                        .elements
                        .iter()
                        .filter_map(|element| {
                            element
                                .text()
                                .map(|text| (element.id, text.translation.as_ref()))
                        })
                        .collect::<Vec<_>>(),
                )?,
                Artifact::Typography => serde_json::to_vec(
                    &page
                        .elements
                        .iter()
                        .filter_map(|element| {
                            element
                                .text()
                                .map(|text| (element.id, &text.style, &text.layout))
                        })
                        .collect::<Vec<_>>(),
                )?,
                Artifact::CleanImage => serde_json::to_vec(&page.assets.clean)?,
            };
            hasher.update(&bytes);
        }
    }
    Ok(*hasher.finalize().as_bytes())
}

fn validate_processor(model: &ConfiguredModel, processor: &dyn Processor) -> Result<()> {
    if processor.inputs() != model.inputs()
        || processor.outputs() != model.outputs()
        || processor.name() != model.name()
    {
        bail!(
            "processor factory returned the wrong processor for {}",
            model.name()
        );
    }
    Ok(())
}

fn capture(
    session: &Session,
    request: &RunRequest,
    cache: &mut HashMap<BlobId, Arc<[u8]>>,
    decoded: Arc<Mutex<HashMap<BlobId, Arc<image::DynamicImage>>>>,
) -> Result<Context> {
    let pages = scoped_pages(session, &request.scope)?;
    let mut ids = BTreeSet::new();
    for page in &pages {
        ids.insert(page.source);
        ids.extend(
            [
                page.assets.clean,
                page.assets.rendered,
                page.assets.text_mask_candidate,
                page.assets.layout_text_mask,
                page.assets.text_mask,
                page.assets.coo_mask,
                page.assets.bubble_mask,
                page.assets.brush_mask,
            ]
            .into_iter()
            .flatten(),
        );
    }
    for id in &ids {
        if !cache.contains_key(id) {
            cache.insert(*id, session.read_blob(*id)?);
        }
    }
    let blobs = ids
        .into_iter()
        .map(|id| (id, BlobBytes::Owned(cache[&id].clone())))
        .collect();
    Ok(Context::new(
        session.revision(),
        request.scope.clone(),
        pages,
        blobs,
        decoded,
        request.context.clone(),
    ))
}

fn scoped_pages(session: &Session, scope: &Scope) -> Result<Vec<Page>> {
    let mut ids = Vec::new();
    match scope {
        Scope::Project => ids.extend(session.project().pages.iter().map(|page| page.id)),
        Scope::Pages { pages } => ids.extend(pages),
        Scope::Region { page, .. } => ids.push(*page),
        Scope::Elements { elements } => {
            for element in elements {
                ids.push(session.element(*element)?.0.id);
            }
        }
    }
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter(|id| seen.insert(*id))
        .map(|id| session.page(id).cloned().map_err(Into::into))
        .collect()
}

fn validate_inputs(model: &ConfiguredModel, context: &Context) -> Result<()> {
    if matches!(context.scope(), Scope::Elements { .. }) && !model.supports_element_scope() {
        bail!("{} does not support an element-only scope", model.name());
    }
    if matches!(
        model,
        ConfiguredModel::Processor(
            ProcessorConfig::LaMa(_)
                | ProcessorConfig::AotInpainting(_)
                | ProcessorConfig::Flux2Klein(_)
                | ProcessorConfig::RoremMixed(_)
        )
    ) {
        for page in context.pages() {
            if page.assets.text_mask.is_none()
                && page.assets.coo_mask.is_none()
                && page.assets.brush_mask.is_none()
            {
                bail!(
                    "page {} has no text, COO, or brush mask for inpainting",
                    page.id
                );
            }
        }
    }
    Ok(())
}

fn validate_commands(
    model: &ConfiguredModel,
    context: &Context,
    commands: &Commands,
) -> Result<()> {
    if commands.base() != context.revision() {
        bail!(
            "{} returned commands for a different revision",
            model.name()
        );
    }
    let inserted = commands
        .as_slice()
        .iter()
        .filter_map(|command| match command {
            Command::InsertElement { page, element, .. } => {
                Some((element.id, (*page, element.frame)))
            }
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    for command in commands.as_slice() {
        let allowed = match command {
            Command::InsertElement { page, element, .. } => {
                let declared = match &element.kind {
                    ElementKind::Text(text) if text.role == TextRole::Onomatopoeia => {
                        model.outputs().contains(&Artifact::CooRegion)
                    }
                    ElementKind::Text(_) => model.outputs().contains(&Artifact::TextRegion),
                    ElementKind::Region(region) if region.kind == RegionKind::Panel => {
                        model.outputs().contains(&Artifact::PanelRegion)
                    }
                    ElementKind::Region(region) if region.kind == RegionKind::Bubble => {
                        model.outputs().contains(&Artifact::BubbleRegion)
                    }
                    ElementKind::Image(_) | ElementKind::Region(_) => false,
                };
                declared && scope_allows_insert(context, *page, element.id, element.frame)
            }
            Command::DeleteElement { page, element } => {
                let declared = context
                    .page(*page)
                    .and_then(|page| page.element(*element))
                    .is_some_and(|element| match &element.kind {
                        ElementKind::Text(text) if text.role == TextRole::Onomatopoeia => {
                            model.outputs().contains(&Artifact::CooRegion)
                        }
                        ElementKind::Text(_) => model.outputs().contains(&Artifact::TextRegion),
                        ElementKind::Region(region) if region.kind == RegionKind::Panel => {
                            model.outputs().contains(&Artifact::PanelRegion)
                        }
                        ElementKind::Region(region) if region.kind == RegionKind::Bubble => {
                            model.outputs().contains(&Artifact::BubbleRegion)
                        }
                        ElementKind::Image(_) | ElementKind::Region(_) => false,
                    });
                declared && scope_allows_element(context, *page, *element)
            }
            Command::EditElement {
                page,
                element,
                edit,
            } => {
                scope_allows_edit(context, *page, *element, &inserted)
                    && match edit {
                        ElementChange::Frame(_) => model.outputs().contains(&Artifact::TextRegion),
                        ElementChange::Source(_) => {
                            model.outputs().contains(&Artifact::SourceText)
                                || model.outputs().contains(&Artifact::CooText)
                                || ((model.outputs().contains(&Artifact::TextRegion)
                                    || model.outputs().contains(&Artifact::CooRegion))
                                    && inserted.contains_key(element))
                        }
                        ElementChange::Translation(value) => {
                            model.outputs().contains(&Artifact::Translation)
                                || (value.is_none()
                                    && (model.outputs().contains(&Artifact::SourceText)
                                        || model.outputs().contains(&Artifact::CooText)))
                        }
                        ElementChange::Style(_) | ElementChange::Layout(_) => {
                            model.outputs().contains(&Artifact::Typography)
                        }
                        ElementChange::Analysis(_) => {
                            model.outputs().contains(&Artifact::CooRegion)
                                || model.outputs().contains(&Artifact::PanelRegion)
                                || model.outputs().contains(&Artifact::BubbleRegion)
                        }
                        _ => false,
                    }
            }
            Command::SetPageAsset {
                page, asset, blob, ..
            } => {
                scope_allows_page_asset(context, *page)
                    && match asset {
                        PageAsset::TextMaskCandidate => {
                            model.outputs().contains(&Artifact::TextMaskCandidate)
                        }
                        PageAsset::LayoutTextMask => {
                            model.outputs().contains(&Artifact::LayoutTextMask)
                        }
                        PageAsset::TextMask => model.outputs().contains(&Artifact::TextMask),
                        PageAsset::CooMask => model.outputs().contains(&Artifact::CooMask),
                        PageAsset::BubbleMask => model.outputs().contains(&Artifact::BubbleMask),
                        PageAsset::Clean => {
                            blob.is_some() && model.outputs().contains(&Artifact::CleanImage)
                        }
                        PageAsset::Rendered | PageAsset::BrushMask => false,
                    }
            }
            _ => false,
        };
        if !allowed {
            bail!("{} emitted an out-of-contract command", model.name());
        }
    }
    Ok(())
}

fn scope_allows_page_asset(context: &Context, page: PageId) -> bool {
    !matches!(context.scope(), Scope::Elements { .. }) && context.page(page).is_some()
}

fn scope_allows_insert(context: &Context, page: PageId, element: ElementId, frame: Frame) -> bool {
    !matches!(context.scope(), Scope::Elements { .. })
        && context.includes_element(page, element, frame)
}

fn scope_allows_element(context: &Context, page: PageId, element: ElementId) -> bool {
    context
        .page(page)
        .and_then(|page| page.element(element))
        .is_some_and(|value| context.includes_element(page, element, value.frame))
}

fn scope_allows_edit(
    context: &Context,
    page: PageId,
    element: ElementId,
    inserted: &HashMap<ElementId, (PageId, Frame)>,
) -> bool {
    scope_allows_element(context, page, element)
        || inserted
            .get(&element)
            .is_some_and(|(inserted_page, frame)| {
                *inserted_page == page && scope_allows_insert(context, page, element, *frame)
            })
}

fn add_invalidations(context: &Context, commands: &mut Commands) {
    let mut clean = BTreeSet::new();
    let mut clean_written = BTreeSet::new();
    let mut rendered = BTreeSet::new();
    let mut text_mask = BTreeSet::new();
    let mut text_mask_written = BTreeSet::new();
    let mut coo_mask = BTreeSet::new();
    let mut coo_mask_written = BTreeSet::new();
    for command in commands.as_slice() {
        match command {
            Command::InsertElement { page, element, .. }
                if matches!(element.kind, ElementKind::Text(_)) =>
            {
                clean.insert(*page);
                rendered.insert(*page);
                text_mask.insert(*page);
                coo_mask.insert(*page);
            }
            Command::DeleteElement { page, element } => {
                if context
                    .page(*page)
                    .and_then(|page| page.element(*element))
                    .is_some_and(|element| matches!(element.kind, ElementKind::Text(_)))
                {
                    clean.insert(*page);
                    rendered.insert(*page);
                    text_mask.insert(*page);
                    coo_mask.insert(*page);
                }
            }
            Command::EditElement {
                page,
                edit:
                    ElementChange::Frame(_)
                    | ElementChange::Source(_)
                    | ElementChange::Translation(_)
                    | ElementChange::Style(_)
                    | ElementChange::Layout(_),
                ..
            } => {
                rendered.insert(*page);
            }
            Command::EditElement {
                page,
                edit: ElementChange::Analysis(_),
                ..
            } => {
                clean.insert(*page);
                rendered.insert(*page);
                text_mask.insert(*page);
                coo_mask.insert(*page);
            }
            Command::SetPageAsset {
                page,
                asset: PageAsset::TextMask,
                ..
            } => {
                clean.insert(*page);
                rendered.insert(*page);
                text_mask_written.insert(*page);
            }
            Command::SetPageAsset {
                page,
                asset: PageAsset::CooMask,
                ..
            } => {
                clean.insert(*page);
                rendered.insert(*page);
                coo_mask_written.insert(*page);
            }
            Command::SetPageAsset {
                page,
                asset: PageAsset::TextMaskCandidate | PageAsset::LayoutTextMask,
                ..
            } => {
                clean.insert(*page);
                rendered.insert(*page);
                text_mask.insert(*page);
                coo_mask.insert(*page);
            }
            Command::SetPageAsset {
                page,
                asset: PageAsset::BubbleMask,
                ..
            } => {
                rendered.insert(*page);
            }
            Command::SetPageAsset {
                page,
                asset: PageAsset::Clean,
                blob,
            } => {
                if blob.is_some() {
                    clean_written.insert(*page);
                }
                rendered.insert(*page);
            }
            _ => {}
        }
    }
    for page in text_mask.difference(&text_mask_written) {
        if context
            .page(*page)
            .is_some_and(|page| page.assets.text_mask.is_some())
        {
            commands.push(Command::SetPageAsset {
                page: *page,
                asset: PageAsset::TextMask,
                blob: None,
            });
        }
    }
    for page in coo_mask.difference(&coo_mask_written) {
        if context
            .page(*page)
            .is_some_and(|page| page.assets.coo_mask.is_some())
        {
            commands.push(Command::SetPageAsset {
                page: *page,
                asset: PageAsset::CooMask,
                blob: None,
            });
        }
    }
    for page in clean.difference(&clean_written) {
        if context
            .page(*page)
            .is_some_and(|page| page.assets.clean.is_some())
        {
            commands.push(Command::SetPageAsset {
                page: *page,
                asset: PageAsset::Clean,
                blob: None,
            });
        }
    }
    for page in rendered {
        if context
            .page(page)
            .is_some_and(|page| page.assets.rendered.is_some())
        {
            commands.push(Command::SetPageAsset {
                page,
                asset: PageAsset::Rendered,
                blob: None,
            });
        }
    }
}

#[cfg(test)]
mod tests;
