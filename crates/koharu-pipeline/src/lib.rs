//! Typed, scene-native model orchestration for Koharu.

mod builtin;
mod config;
mod context;
mod plan;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    error::Error as StdError,
    fmt,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::join_all;
use koharu_config::Config;
use koharu_ml::Device;
use koharu_scene::{
    BlobId, Command, Commands, ElementChange, ElementId, ElementKind, Frame, Page, PageAsset,
    PageId, Revision, Session,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Mutex as AsyncMutex;

pub use config::*;
pub use context::Context;

use builtin::BuiltinFactory;
use context::ContextOptions;
use plan::{ConfiguredModel, NodeKey, Output, Plan, PlanNode, Selection};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Detection,
    Segmentation,
    Ocr,
    Translation,
    Typography,
    Inpainting,
}

impl fmt::Display for Stage {
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

impl Stage {
    pub const ALL: [Self; 6] = [
        Self::Detection,
        Self::Segmentation,
        Self::Ocr,
        Self::Translation,
        Self::Typography,
        Self::Inpainting,
    ];
}

impl FromStr for Stage {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "detection" | "detect" => Ok(Self::Detection),
            "segmentation" | "segment" => Ok(Self::Segmentation),
            "ocr" => Ok(Self::Ocr),
            "translation" | "translate" => Ok(Self::Translation),
            "typography" | "type" => Ok(Self::Typography),
            "inpainting" | "inpaint" => Ok(Self::Inpainting),
            _ => bail!("unknown pipeline stage '{value}'"),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum Scope {
    #[default]
    Project,
    Pages(Vec<PageId>),
    Region {
        page: PageId,
        frame: Frame,
    },
    Elements(Vec<ElementId>),
}

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub stage: Stage,
    pub model: String,
    pub completed: usize,
    pub total: usize,
}

pub type ProgressSink = Arc<dyn Fn(Progress) + Send + Sync>;

#[async_trait]
pub trait Processor: Send {
    fn name(&self) -> &'static str;
    fn stage(&self) -> Stage;
    async fn run(&mut self, context: &Context) -> Result<Commands>;
}

#[async_trait]
trait ProcessorFactory: Send + Sync {
    async fn load(&self, model: &ConfiguredModel, device: Device) -> Result<Box<dyn Processor>>;
}

struct LoadedProcessor {
    model: ConfiguredModel,
    processor: Arc<AsyncMutex<Box<dyn Processor>>>,
}

pub struct Pipeline {
    config: Config<PipelineConfig>,
    device: Device,
    factory: Arc<dyn ProcessorFactory>,
    loaded: AsyncMutex<BTreeMap<NodeKey, LoadedProcessor>>,
    accelerator: AsyncMutex<()>,
    run_lock: AsyncMutex<()>,
}

impl Pipeline {
    #[must_use]
    pub fn new(config: impl Into<Config<PipelineConfig>>) -> Self {
        Self::with_factory(config.into(), Arc::new(BuiltinFactory))
    }

    fn with_factory(config: Config<PipelineConfig>, factory: Arc<dyn ProcessorFactory>) -> Self {
        Self {
            config,
            device: koharu_ml::device(false),
            factory,
            loaded: AsyncMutex::new(BTreeMap::new()),
            accelerator: AsyncMutex::new(()),
            run_lock: AsyncMutex::new(()),
        }
    }

    pub fn check(&self) -> Result<()> {
        let config = self.config.read()?.clone();
        Plan::build(&config, Selection::All).map(|_| ())
    }

    pub fn graph(&self) -> Result<String> {
        let config = self.config.read()?.clone();
        Ok(Plan::build(&config, Selection::All)?.dot())
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

    pub async fn load(&self, stage: Stage) -> Result<()> {
        let _run = self.run_lock.lock().await;
        let config = self.config.read()?.clone();
        let all = Plan::build(&config, Selection::All)?;
        self.reconcile(&all).await;
        let nodes = all
            .nodes
            .iter()
            .filter(|node| node.key.stage == stage)
            .collect::<Vec<_>>();
        if nodes.is_empty() {
            bail!("stage {stage} has no configured model");
        }
        self.ensure_loaded(&nodes).await.map(|_| ())
    }

    pub async fn unload(&self, stage: Stage) -> Result<()> {
        let _run = self.run_lock.lock().await;
        self.loaded.lock().await.retain(|key, _| key.stage != stage);
        Ok(())
    }

    pub async fn unload_all(&self) -> Result<()> {
        let _run = self.run_lock.lock().await;
        self.loaded.lock().await.clear();
        Ok(())
    }

    async fn execute(
        &self,
        session: &mut Session,
        request: RunRequest,
    ) -> std::result::Result<RunReport, RunError> {
        let mut revisions = Vec::new();
        let result = self.execute_inner(session, request, &mut revisions).await;
        match result {
            Ok(processors) => Ok(RunReport {
                revisions,
                processors,
            }),
            Err(source) => Err(RunError {
                source,
                committed_revisions: revisions,
            }),
        }
    }

    async fn execute_inner(
        &self,
        session: &mut Session,
        request: RunRequest,
        revisions: &mut Vec<Revision>,
    ) -> Result<usize> {
        let _run = self.run_lock.lock().await;
        let config = self.config.read()?.clone();
        let all = Plan::build(&config, Selection::All)?;
        self.reconcile(&all).await;
        let plan = Plan::build(&config, request.selection)?;
        let mut blobs = HashMap::new();
        let decoded = Arc::new(Mutex::new(HashMap::new()));
        let completed = AtomicUsize::new(0);

        for wave in &plan.waves {
            if request.cancellation.is_cancelled() {
                bail!("pipeline run was cancelled");
            }
            let context = Arc::new(capture(session, &request, &mut blobs, decoded.clone())?);
            context.validate_scope()?;
            let nodes = wave
                .iter()
                .map(|&index| &plan.nodes[index])
                .collect::<Vec<_>>();
            for node in &nodes {
                validate_inputs(&node.model, &context)?;
            }
            let processors = self.ensure_loaded(&nodes).await?;
            let futures = nodes.iter().zip(processors).map(|(node, processor)| {
                let context = context.clone();
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
            if request.cancellation.is_cancelled() {
                bail!("pipeline run was cancelled");
            }
            add_invalidations(&context, &mut merged);
            if !merged.as_slice().is_empty() {
                let change = session.apply(merged)?;
                if change.to != change.from {
                    revisions.push(change.to);
                }
            }
            for node in nodes {
                let current = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(progress) = &request.progress {
                    progress(Progress {
                        stage: node.key.stage,
                        model: node.model.name().to_owned(),
                        completed: current,
                        total: plan.nodes.len(),
                    });
                }
            }
        }
        Ok(plan.nodes.len())
    }

    async fn reconcile(&self, plan: &Plan) {
        let wanted = plan
            .nodes
            .iter()
            .map(|node| (node.key, &node.model))
            .collect::<BTreeMap<_, _>>();
        self.loaded
            .lock()
            .await
            .retain(|key, loaded| wanted.get(key).is_some_and(|model| **model == loaded.model));
    }

    async fn ensure_loaded(
        &self,
        nodes: &[&PlanNode],
    ) -> Result<Vec<Arc<AsyncMutex<Box<dyn Processor>>>>> {
        let missing = {
            let loaded = self.loaded.lock().await;
            nodes
                .iter()
                .filter(|node| !loaded.contains_key(&node.key))
                .map(|node| (node.key, node.model.clone()))
                .collect::<Vec<_>>()
        };
        let loads = missing.iter().map(|(_, model)| {
            let factory = self.factory.clone();
            let device = self.device.clone();
            async move { factory.load(model, device).await }
        });
        let loaded_processors = join_all(loads).await;
        if !missing.is_empty() {
            let mut loaded = self.loaded.lock().await;
            for ((key, model), processor) in missing.into_iter().zip(loaded_processors) {
                let processor = processor?;
                if processor.stage() != model.stage() || processor.name() != model.name() {
                    bail!(
                        "processor factory returned the wrong processor for {}",
                        model.name()
                    );
                }
                loaded.insert(
                    key,
                    LoadedProcessor {
                        model,
                        processor: Arc::new(AsyncMutex::new(processor)),
                    },
                );
            }
        }
        let loaded = self.loaded.lock().await;
        nodes
            .iter()
            .map(|node| {
                loaded
                    .get(&node.key)
                    .map(|value| value.processor.clone())
                    .ok_or_else(|| anyhow!("{} was not loaded", node.model.name()))
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StageSelection {
    #[default]
    All,
    Through(Stage),
    Only(Stage),
}

pub fn selected_stages(config: &PipelineConfig, selection: StageSelection) -> Result<Vec<Stage>> {
    let selection = match selection {
        StageSelection::All => Selection::All,
        StageSelection::Through(stage) => Selection::Through(stage),
        StageSelection::Only(stage) => Selection::Only(stage),
    };
    let plan = Plan::build(config, selection)?;
    Ok(plan
        .waves
        .iter()
        .flatten()
        .map(|&index| plan.nodes[index].key.stage)
        .collect())
}

#[derive(Clone)]
struct RunRequest {
    scope: Scope,
    selection: Selection,
    target_language: Option<String>,
    instructions: Option<String>,
    cancellation: CancellationToken,
    progress: Option<ProgressSink>,
}

impl Default for RunRequest {
    fn default() -> Self {
        Self {
            scope: Scope::Project,
            selection: Selection::All,
            target_language: None,
            instructions: None,
            cancellation: CancellationToken::default(),
            progress: None,
        }
    }
}

pub struct Run<'pipeline, 'session> {
    pipeline: &'pipeline Pipeline,
    session: &'session mut Session,
    request: RunRequest,
}

impl Run<'_, '_> {
    #[must_use]
    pub fn pages(mut self, pages: impl IntoIterator<Item = PageId>) -> Self {
        self.request.scope = Scope::Pages(pages.into_iter().collect());
        self
    }

    #[must_use]
    pub fn region(mut self, page: PageId, frame: Frame) -> Self {
        self.request.scope = Scope::Region { page, frame };
        self
    }

    #[must_use]
    pub fn elements(mut self, elements: impl IntoIterator<Item = ElementId>) -> Self {
        self.request.scope = Scope::Elements(elements.into_iter().collect());
        self
    }

    #[must_use]
    pub fn through(mut self, stage: Stage) -> Self {
        self.request.selection = Selection::Through(stage);
        self
    }

    #[must_use]
    pub fn only(mut self, stage: Stage) -> Self {
        self.request.selection = Selection::Only(stage);
        self
    }

    #[must_use]
    pub fn target_language(mut self, language: impl Into<String>) -> Self {
        self.request.target_language = Some(language.into());
        self
    }

    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.request.instructions = Some(instructions.into());
        self
    }

    #[must_use]
    pub fn cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.request.cancellation = cancellation;
        self
    }

    #[must_use]
    pub fn progress(mut self, progress: ProgressSink) -> Self {
        self.request.progress = Some(progress);
        self
    }

    pub async fn execute(self) -> std::result::Result<RunReport, RunError> {
        self.pipeline.execute(self.session, self.request).await
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunReport {
    pub revisions: Vec<Revision>,
    pub processors: usize,
}

#[derive(Debug)]
pub struct RunError {
    source: anyhow::Error,
    pub committed_revisions: Vec<Revision>,
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl StdError for RunError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.source()
    }
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
                page.assets.text_mask,
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
    let blobs = ids.into_iter().map(|id| (id, cache[&id].clone())).collect();
    Ok(Context::new(
        session.revision(),
        request.scope.clone(),
        pages,
        blobs,
        decoded,
        ContextOptions {
            target_language: request.target_language.clone(),
            instructions: request.instructions.clone(),
            cancellation: request.cancellation.clone(),
        },
    ))
}

fn scoped_pages(session: &Session, scope: &Scope) -> Result<Vec<Page>> {
    let mut ids = Vec::new();
    match scope {
        Scope::Project => ids.extend(session.project().pages.iter().map(|page| page.id)),
        Scope::Pages(pages) => ids.extend(pages),
        Scope::Region { page, .. } => ids.push(*page),
        Scope::Elements(elements) => {
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
    if matches!(context.scope(), Scope::Elements(_))
        && matches!(
            model.stage(),
            Stage::Detection | Stage::Segmentation | Stage::Inpainting
        )
    {
        bail!("{} does not support an element-only scope", model.name());
    }
    if matches!(model, ConfiguredModel::Inpainting(_)) {
        for page in context.pages() {
            if page.assets.text_mask.is_none() && page.assets.brush_mask.is_none() {
                bail!("page {} has no text or brush mask for inpainting", page.id);
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
                model.outputs().contains(&Output::Text)
                    && matches!(element.kind, ElementKind::Text(_))
                    && scope_allows_insert(context, *page, element.id, element.frame)
            }
            Command::DeleteElement { page, element } => {
                model.outputs().contains(&Output::Text)
                    && scope_allows_element(context, *page, *element)
            }
            Command::EditElement {
                page,
                element,
                edit,
            } => {
                scope_allows_edit(context, *page, *element, &inserted)
                    && match edit {
                        ElementChange::Frame(_) => model.outputs().contains(&Output::Text),
                        ElementChange::Source(_) => {
                            model.outputs().contains(&Output::SourceText)
                                || (model.outputs().contains(&Output::Text)
                                    && inserted.contains_key(element))
                        }
                        ElementChange::Translation(value) => {
                            model.outputs().contains(&Output::Translation)
                                || (value.is_none()
                                    && model.outputs().contains(&Output::SourceText))
                        }
                        ElementChange::Style(_) | ElementChange::Layout(_) => {
                            model.outputs().contains(&Output::Typography)
                        }
                        _ => false,
                    }
            }
            Command::SetPageAsset {
                page, asset, blob, ..
            } => {
                scope_allows_page_asset(context, *page)
                    && match asset {
                        PageAsset::TextMask => model.outputs().contains(&Output::TextMask),
                        PageAsset::BubbleMask => model.outputs().contains(&Output::BubbleMask),
                        PageAsset::Clean => {
                            blob.is_some() && model.outputs().contains(&Output::Clean)
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
    !matches!(context.scope(), Scope::Elements(_)) && context.page(page).is_some()
}

fn scope_allows_insert(context: &Context, page: PageId, element: ElementId, frame: Frame) -> bool {
    !matches!(context.scope(), Scope::Elements(_)) && context.includes_element(page, element, frame)
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
    for command in commands.as_slice() {
        match command {
            Command::InsertElement { page, element, .. }
                if matches!(element.kind, ElementKind::Text(_)) =>
            {
                clean.insert(*page);
                rendered.insert(*page);
                text_mask.insert(*page);
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
