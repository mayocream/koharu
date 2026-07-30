use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow, bail};
use futures::future::join_all;
use koharu_scene::{BlobId, Page, Revision, Session};

use crate::{
    BlobBytes, Context, Pipeline, PipelineEvent, Processor, ProcessorEntry, Progress, RunError,
    RunReport, RunRequest, Scope,
    plan::{Plan, PlanNode},
};

impl Pipeline {
    pub(crate) async fn execute(
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
            Ok(processors) => Ok(RunReport {
                revisions,
                processors,
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
    ) -> Result<usize> {
        let _run = self.run_lock.lock().await;
        let config = self.config.read()?.clone();
        let translation = self.translation.read()?.clone();
        request.context.translation = crate::context::TranslationOptions {
            target_language: translation.target_language,
            instructions: translation.instructions,
        };
        let all = Plan::build(&config, &translation.model)?;
        self.reconcile(&all).await;
        let plan = all.select(&request.target)?;
        let total = plan.nodes().count();
        let mut blobs = HashMap::new();
        let decoded = Arc::new(Mutex::new(HashMap::new()));
        let completed = AtomicUsize::new(0);

        for wave in plan.waves() {
            if request.context.cancellation.is_cancelled() {
                bail!("pipeline run was cancelled");
            }
            let context = Arc::new(capture(session, &request, &mut blobs, decoded.clone())?);
            context.validate_scope()?;
            let nodes = wave
                .iter()
                .map(|index| plan.node(*index))
                .collect::<Vec<_>>();
            for node in &nodes {
                if matches!(request.scope, Scope::Elements { .. })
                    && !node.node.spec().supports_element_scope
                {
                    bail!(
                        "{} does not support an element-only scope",
                        node.node.name()
                    );
                }
            }
            let processors = self.ensure_processors(&nodes).await?;
            let futures = nodes.iter().zip(processors).map(|(node, processor)| {
                let context = Arc::new(context.for_phase(node.phase()));
                async move {
                    if context.cancellation().is_cancelled() {
                        bail!("pipeline run was cancelled");
                    }
                    let _accelerator = if node.node.uses_accelerator()
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
                        .with_context(|| format!("{} failed", node.node.name()))
                }
            });
            let results = join_all(futures).await;
            let mut merged = context.commands();
            for (node, result) in nodes.iter().zip(results) {
                merged.merge(result?).with_context(|| {
                    format!("{} produced conflicting commands", node.node.name())
                })?;
            }
            if request.context.cancellation.is_cancelled() {
                bail!("pipeline run was cancelled");
            }
            if !merged.as_slice().is_empty() {
                let change = session.apply(merged)?;
                if change.to != change.from {
                    revisions.push(change.to);
                }
            }
            for node in nodes {
                let current = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(events) = &request.context.events {
                    events(PipelineEvent::Progress(Progress {
                        phase: node.phase(),
                        model: node.node.name().to_owned(),
                        completed: current,
                        total,
                    }));
                }
            }
        }
        Ok(total)
    }

    async fn reconcile(&self, plan: &Plan) {
        let wanted = plan
            .nodes()
            .map(|(_, node)| (node.id(), &node.node))
            .collect::<BTreeMap<_, _>>();
        let removed = {
            let mut processors = self.processors.lock().await;
            let keys = processors
                .iter()
                .filter_map(|(key, entry)| {
                    (!wanted.get(key).is_some_and(|node| **node == entry.node)).then_some(*key)
                })
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| processors.remove(&key))
                .collect::<Vec<_>>()
        };
        Self::shutdown_loaded(removed).await;
    }

    pub(crate) async fn shutdown_loaded(processors: impl IntoIterator<Item = ProcessorEntry>) {
        for processor in processors {
            processor.processor.lock().await.shutdown().await;
        }
    }

    async fn ensure_processors(
        &self,
        nodes: &[&PlanNode],
    ) -> Result<Vec<Arc<tokio::sync::Mutex<Box<dyn Processor>>>>> {
        let missing = {
            let processors = self.processors.lock().await;
            nodes
                .iter()
                .filter(|node| !processors.contains_key(&node.id()))
                .map(|node| (node.id(), node.node.clone()))
                .collect::<Vec<_>>()
        };
        let loads = missing.iter().map(|(_, node)| {
            let factory = self.factory.clone();
            let device = self.device.clone();
            async move { factory.create(node, device).await }
        });
        let loaded = join_all(loads).await;
        if !missing.is_empty() {
            let mut processors = self.processors.lock().await;
            for ((id, node), processor) in missing.into_iter().zip(loaded) {
                let processor = processor?;
                processors.insert(
                    id,
                    ProcessorEntry {
                        node,
                        processor: Arc::new(tokio::sync::Mutex::new(processor)),
                    },
                );
            }
        }
        let processors = self.processors.lock().await;
        nodes
            .iter()
            .map(|node| {
                processors
                    .get(&node.id())
                    .map(|value| value.processor.clone())
                    .ok_or_else(|| anyhow!("{} was not loaded", node.node.name()))
            })
            .collect()
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
