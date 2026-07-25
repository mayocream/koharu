//! Engine trait + inventory-based registry + DAG resolver.
//!
//! An engine is a pluggable model that transforms one page. It declares the
//! artifacts it needs and produces; the DAG resolver derives execution order.
//!
//! **Engines emit ops, not mutations.** `run()` returns `Vec<Op>`; the driver
//! wraps them in `Op::Batch` and hands to `ProjectSession::apply`.
//!
//! ## Adding an engine
//!
//! 1. Define a struct holding your model.
//! 2. Implement `Engine` for it (returning `Vec<Op>`).
//! 3. Register via `inventory::submit! { EngineInfo { … } }` with a static
//!    async `load` function.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Result, bail};
use async_trait::async_trait;
use koharu_core::{NodeId, Op, PageId, ReadingOrder, Region, Scene};
use koharu_runtime::RuntimeManager;
use parking_lot::RwLock;
use petgraph::algo::toposort;
use petgraph::graph::DiGraph;
use tracing::Instrument;

use crate::blobs::BlobStore;
use crate::llm;
use crate::pipeline::artifacts::Artifact;
use crate::renderer;

// ---------------------------------------------------------------------------
// EngineCtx — everything an engine needs to produce ops
// ---------------------------------------------------------------------------

pub struct EngineCtx<'a> {
    /// A cheap clone of the target page (read-only).
    pub scene: &'a Scene,
    pub page: PageId,
    pub blobs: &'a BlobStore,
    pub runtime: &'a RuntimeManager,
    pub cancel: &'a AtomicBool,
    pub options: &'a PipelineRunOptions,
    pub llm: &'a llm::Model,
    pub renderer: &'a renderer::Renderer,
}

/// Options threaded through a pipeline run.
#[derive(Debug, Clone, Default)]
pub struct PipelineRunOptions {
    pub target_language: Option<String>,
    pub system_prompt: Option<String>,
    pub default_font: Option<String>,
    /// Optional text-node scope for engines that can operate on individual
    /// text blocks. Engines that render full-page artifacts ignore it.
    pub text_node_ids: Option<Vec<NodeId>>,
    /// Optional bounding-box hint. Inpainter engines (lama/aot) honor it:
    /// composite onto the existing `Image { Inpainted }` (fallback Source)
    /// and process just that one block. Other engines ignore it.
    pub region: Option<Region>,
    pub reading_order: Option<ReadingOrder>,
}

// ---------------------------------------------------------------------------
// Engine trait
// ---------------------------------------------------------------------------

/// Runtime facts the driver hands each engine so it can size its own
/// concurrency. Built once per pipeline run.
#[derive(Debug, Clone, Copy)]
pub struct ConcurrencyHint {
    /// Suggested worker count for CPU-bound, lock-free stages.
    pub cpu_workers: usize,
    /// True when the loaded translator is a remote HTTP provider (network
    /// bound, safe to fan out) rather than a local llama.cpp context
    /// (single-context, `&mut self`, cannot be parallelized).
    pub translator_is_remote: bool,
    /// True when a user-supplied system prompt is in effect. Such a prompt
    /// describes the single-page `[N]` block format and cannot be assumed to
    /// teach the batched `[bP-N]` form, so cross-page translation batching
    /// must be disabled.
    pub custom_system_prompt: bool,
    /// Hard cap on pages folded into one model call. 1 disables batching.
    pub max_batch_pages: usize,
}

#[async_trait]
pub trait Engine: Send + Sync + 'static {
    /// Run the engine on one page. Return the ops to apply.
    /// Empty `Vec` = nothing changed (still a success).
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>>;

    /// How many pages this engine can safely process concurrently.
    ///
    /// Default 1. Only override when the engine holds no exclusive lock and
    /// contends for a resource that actually parallelizes — CPU cores or the
    /// network. Fanning out a stage that shares one GPU or one `&mut` model
    /// buys nothing and multiplies peak memory.
    fn max_workers(&self, _hint: &ConcurrencyHint) -> usize {
        1
    }

    /// How many pages this engine can fold into a single model call.
    ///
    /// Default 1 (no batching). Override only where the underlying model
    /// genuinely batches — a real tensor batch or a single combined request —
    /// not where the "batch" API just loops internally.
    fn max_batch(&self, _hint: &ConcurrencyHint) -> usize {
        1
    }

    /// Run the engine over several pages at once.
    ///
    /// Returns **one result per input context, in the same order**, so a
    /// failure is attributed to the page that caused it rather than failing
    /// the whole group. The default implementation simply runs them in
    /// sequence, so engines that don't override `max_batch` never see a
    /// batch larger than 1 and need not implement this.
    async fn run_batch(&self, ctxs: Vec<EngineCtx<'_>>) -> Vec<Result<Vec<Op>>> {
        let mut out = Vec::with_capacity(ctxs.len());
        for ctx in ctxs {
            out.push(self.run(ctx).await);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// EngineInfo — static descriptor + factory (registered via inventory)
// ---------------------------------------------------------------------------

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type EngineLoadFn =
    for<'a> fn(&'a RuntimeManager, bool) -> BoxFuture<'a, Result<Box<dyn Engine>>>;

pub struct EngineInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub needs: &'static [Artifact],
    pub produces: &'static [Artifact],
    pub load: EngineLoadFn,
}

inventory::collect!(EngineInfo);

// ---------------------------------------------------------------------------
// Registry — lazy load + cache engine instances
// ---------------------------------------------------------------------------

pub struct Registry {
    engines: RwLock<HashMap<&'static str, Arc<dyn Engine>>>,
    /// One lock per engine id, held across the `load` await so that
    /// concurrent misses for the same engine don't each allocate a copy of
    /// the model (and its GPU memory). Only ever guards loading; lookups of
    /// already-cached engines never touch it.
    loading: tokio::sync::Mutex<HashMap<&'static str, Arc<tokio::sync::Mutex<()>>>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            engines: RwLock::new(HashMap::new()),
            loading: tokio::sync::Mutex::new(HashMap::new()),
        }
    }
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or load an engine instance by id.
    ///
    /// Safe to call concurrently for the same id: only one caller performs the
    /// load, the rest wait and observe the cached instance. Without this,
    /// parallel pipeline stages hitting a cold engine would each load the
    /// model and allocate its GPU memory, then discard all but one.
    pub async fn get(
        &self,
        id: &str,
        runtime: &RuntimeManager,
        cpu: bool,
    ) -> Result<Arc<dyn Engine>> {
        if let Some(engine) = self.engines.read().get(id).cloned() {
            return Ok(engine);
        }
        let info = Self::find(id)?;

        // Take this engine's load lock. Scoped so the map lock isn't held
        // across the load itself.
        let load_lock = {
            let mut loading = self.loading.lock().await;
            loading.entry(info.id).or_default().clone()
        };
        let _guard = load_lock.lock().await;

        // Re-check: another caller may have loaded it while we waited.
        if let Some(engine) = self.engines.read().get(info.id).cloned() {
            return Ok(engine);
        }

        let loaded = async { (info.load)(runtime, cpu).await }
            .instrument(tracing::info_span!("engine_load", engine = id))
            .await?;
        let engine: Arc<dyn Engine> = Arc::from(loaded);
        self.engines.write().insert(info.id, engine.clone());
        Ok(engine)
    }

    /// Drop all cached engines (frees GPU memory).
    pub fn clear(&self) {
        self.engines.write().clear();
    }

    /// Find engine descriptor by id.
    pub fn find(id: &str) -> Result<&'static EngineInfo> {
        Self::catalog()
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown engine: {id}"))
    }

    /// All registered engine descriptors.
    pub fn catalog() -> Vec<&'static EngineInfo> {
        inventory::iter::<EngineInfo>.into_iter().collect()
    }

    /// Engines that produce a given artifact.
    pub fn providers(artifact: Artifact) -> Vec<&'static EngineInfo> {
        Self::catalog()
            .into_iter()
            .filter(|e| e.produces.contains(&artifact))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// DAG — derive execution order from artifact dependencies
// ---------------------------------------------------------------------------

/// Build a topological execution order from a set of engine infos.
pub fn build_order(infos: &[&EngineInfo]) -> Result<Vec<usize>> {
    let mut g = DiGraph::<usize, ()>::new();
    let mut id_to_node: HashMap<&str, _> = HashMap::new();

    for (i, info) in infos.iter().enumerate() {
        let n = g.add_node(i);
        if id_to_node.insert(info.id, n).is_some() {
            bail!("duplicate engine: {}", info.id);
        }
    }

    let mut producers: HashMap<Artifact, usize> = HashMap::new();
    for (i, info) in infos.iter().enumerate() {
        for &artifact in info.produces {
            producers.insert(artifact, i);
        }
    }

    for info in infos.iter() {
        let to = id_to_node[info.id];
        for &artifact in info.needs {
            if let Some(&producer) = producers.get(&artifact) {
                g.add_edge(id_to_node[infos[producer].id], to, ());
            }
        }
    }

    let order = toposort(&g, None)
        .map_err(|c| anyhow::anyhow!("cycle at '{}'", infos[g[c.node_id()]].id))?;
    Ok(order.into_iter().map(|n| g[n]).collect())
}
