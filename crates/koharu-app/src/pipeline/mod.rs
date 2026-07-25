//! Pipeline: runs an ordered set of engines across one or more pages and
//! wraps each engine's output in one `Op::Batch` before applying via the
//! session's history.
//!
//! **Engines don't mutate the scene.** They return `Vec<Op>`; this driver
//! applies them transactionally (per-engine) against the active session.

pub mod artifacts;
pub mod engine;
mod engines;

pub use artifacts::Artifact;
pub use engine::{
    BoxFuture, ConcurrencyHint, Engine, EngineCtx, EngineInfo, EngineLoadFn, PipelineRunOptions,
    Registry, build_order,
};
pub use engines::support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::{Result, bail};
use koharu_core::{Op, PageId, PipelineStep};
use koharu_runtime::RuntimeManager;
use tracing::Instrument;

/// Observer for pipeline progress. `step_id` is the engine id of the step
/// about to run (or just finished); step_index / page_index are 0-based.
pub type ProgressSink = Arc<dyn Fn(ProgressTick) + Send + Sync>;

/// Observer for non-fatal step failures. Called once per failed step; the
/// pipeline skips the rest of that page's steps and moves on to the next
/// page.
pub type WarningSink = Arc<dyn Fn(WarningTick) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ProgressTick {
    /// Coarse UI-facing step tag derived from the engine's primary
    /// produced artifact. `None` for the final 100% tick where no engine
    /// is running.
    pub step: Option<PipelineStep>,
    /// Engine id (e.g. `"paddle-ocr-vl-1.6"`) for diagnostics + logs.
    pub step_id: String,
    pub step_index: usize,
    pub total_steps: usize,
    pub page_index: usize,
    pub total_pages: usize,
    pub overall_percent: u8,
}

#[derive(Debug, Clone)]
pub struct WarningTick {
    pub step_id: String,
    pub page_index: usize,
    pub total_pages: usize,
    pub message: String,
}

/// Returned by [`run`]. `warning_count == 0` means the run finished cleanly.
#[derive(Debug, Clone, Default)]
pub struct RunOutcome {
    pub warning_count: usize,
}

/// Map an engine's produced artifact to its UI step category. Stays
/// co-located with the engine metadata so adding a new engine can't
/// silently bypass the toolbar spinner — only the registered artifact
/// matters, not the engine's string id.
fn step_for(info: &EngineInfo) -> Option<PipelineStep> {
    info.produces.iter().find_map(|a| match a {
        Artifact::TextBoxes
        | Artifact::SegmentMask
        | Artifact::FontPredictions
        | Artifact::BubbleMask => Some(PipelineStep::Detect),
        Artifact::OcrText => Some(PipelineStep::Ocr),
        Artifact::Translations => Some(PipelineStep::LlmGenerate),
        Artifact::Inpainted => Some(PipelineStep::Inpaint),
        Artifact::FinalRender => Some(PipelineStep::Render),
        // Non-UI-facing artifacts (inputs, intermediate sprites) — no
        // toolbar step tag.
        _ => None,
    })
}

use crate::llm;
use crate::renderer;
use crate::session::ProjectSession;

// ---------------------------------------------------------------------------
// Spec + scope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PipelineSpec {
    pub scope: Scope,
    pub steps: Vec<String>,
    pub options: PipelineRunOptions,
    /// Concurrency caps. `Default` means auto — see [`PipelineLimits`].
    pub limits: PipelineLimits,
}

/// Workers a CPU-bound, lock-free stage gets when sizing itself automatically.
///
/// Capped well below the core count because the stages that use it run
/// alongside GPU stages and the HTTP server, and because each worker holds a
/// page's worth of pixels. Exposed so the settings UI can show what auto
/// resolves to on this machine.
pub fn auto_cpu_workers() -> usize {
    num_cpus::get().clamp(1, 4)
}

/// Caps on how much of the pipeline runs at once. Both `0` mean "auto".
#[derive(Debug, Clone, Copy, Default)]
pub struct PipelineLimits {
    /// Pages allowed in the pipeline simultaneously. `1` restores fully
    /// sequential processing (and forces every batch to size 1).
    pub max_inflight_pages: usize,
    /// Cap on pages any stage folds into one model call. `1` disables
    /// batching while keeping stage overlap.
    pub max_batch_pages: usize,
}

#[derive(Debug, Clone)]
pub enum Scope {
    WholeProject,
    Pages(Vec<PageId>),
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// Execute `spec` against `session`. Each engine step becomes one `Op::Batch`
/// applied via the session's history (one undo step per step per page).
///
/// A failed step on a given page is non-fatal: the rest of that page's steps
/// are skipped (they typically depend on the failed step's output), one
/// [`WarningTick`] is emitted via `warnings`, and the driver moves on to the
/// next page. The function returns the total number of per-step warnings
/// that fired, letting callers flag the run as `CompletedWithErrors`.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(level = "info", skip_all)]
pub async fn run(
    session: Arc<ProjectSession>,
    registry: Arc<Registry>,
    runtime: Arc<RuntimeManager>,
    cpu: bool,
    llm: Arc<llm::Model>,
    renderer: Arc<renderer::Renderer>,
    spec: PipelineSpec,
    cancel: Arc<AtomicBool>,
    progress: Option<ProgressSink>,
    warnings: Option<WarningSink>,
) -> Result<RunOutcome> {
    let infos: Vec<&EngineInfo> = spec
        .steps
        .iter()
        .map(|id| Registry::find(id))
        .collect::<Result<_>>()?;
    let order = build_order(&infos)?;

    let pages = match &spec.scope {
        Scope::WholeProject => session
            .scene
            .read()
            .pages
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        Scope::Pages(ids) => ids.clone(),
    };

    let total_pages = pages.len().max(1);
    let total_steps = order.len().max(1);

    // No steps means no stages to wire up. The sequential driver silently did
    // nothing here, so keep that rather than building an empty pipeline.
    if order.is_empty() {
        if let Some(sink) = progress.as_ref() {
            sink(ProgressTick {
                step: None,
                step_id: String::new(),
                step_index: 0,
                total_steps,
                page_index: total_pages.saturating_sub(1),
                total_pages,
                overall_percent: 100,
            });
        }
        return Ok(RunOutcome::default());
    }

    // --- sizing -----------------------------------------------------------

    let max_batch_pages = match spec.limits.max_batch_pages {
        0 => usize::MAX,
        n => n,
    };
    let hint = ConcurrencyHint {
        cpu_workers: auto_cpu_workers(),
        translator_is_remote: llm
            .current_target()
            .await
            .is_some_and(|t| t.kind != koharu_core::LlmTargetKind::Local),
        custom_system_prompt: spec
            .options
            .system_prompt
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty()),
        max_batch_pages,
    };
    // One page in flight means the pipeline is sequential again, so no stage
    // can ever accumulate a batch either.
    let inflight = match spec.limits.max_inflight_pages {
        0 => order.len().max(1),
        n => n,
    };
    let batching_possible = inflight > 1;

    // --- load engines -----------------------------------------------------
    //
    // Up front rather than lazily: `max_workers` / `max_batch` are engine
    // methods, so an instance is needed before its stage can be sized. A load
    // failure is kept non-fatal — that stage warns and drops each page, which
    // is what the sequential driver did per page.

    let mut stages: Vec<StageSpec> = Vec::with_capacity(order.len());
    for &i in &order {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let info = infos[i];
        match registry.get(info.id, &runtime, cpu).await {
            Ok(engine) => {
                let workers = engine.max_workers(&hint).max(1);
                let batch = if batching_possible {
                    engine.max_batch(&hint).clamp(1, max_batch_pages)
                } else {
                    1
                };
                stages.push(StageSpec {
                    info,
                    engine: Some(engine),
                    workers,
                    batch,
                });
            }
            Err(err) => {
                tracing::warn!(engine = info.id, "engine failed to load: {err:#}");
                stages.push(StageSpec {
                    info,
                    engine: None,
                    workers: 1,
                    batch: 1,
                });
            }
        }
    }

    let tracker = Arc::new(Tracker::new(total_pages, total_steps));
    let ctx = Arc::new(StageContext {
        session,
        runtime,
        llm,
        renderer,
        cancel: cancel.clone(),
        options: spec.options,
        progress,
        warnings,
        tracker: tracker.clone(),
    });

    // --- wire the stages together -----------------------------------------

    // Built back-to-front so each stage already knows the channel it forwards
    // into: `senders` holds them in reverse, so the last one pushed is always
    // the immediate downstream stage.
    let mut senders: Vec<async_channel::Sender<Item>> = Vec::with_capacity(stages.len());
    let mut workers = Vec::new();

    for (seq, stage) in stages.iter().enumerate().rev() {
        let (tx, rx) = async_channel::bounded::<Item>(stage.batch.max(2));
        // The final stage has nowhere to forward to; its items are dropped,
        // which is what releases their in-flight permits.
        let forward = senders.last().cloned();

        for _ in 0..stage.workers {
            workers.push(spawn_stage_worker(
                ctx.clone(),
                stage.clone_for_worker(),
                seq,
                rx.clone(),
                forward.clone(),
            ));
        }
        senders.push(tx);
    }

    // Last pushed is stage 0's sender — the head of the pipeline.
    let head = senders.pop().expect("at least one stage");
    // Drop the driver's remaining copies so each stage's channel closes once
    // every upstream worker has finished with it.
    drop(senders);

    // --- feed pages -------------------------------------------------------

    let permits = Arc::new(tokio::sync::Semaphore::new(inflight));
    for (page_index, page_id) in pages.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(permit) = permits.clone().acquire_owned().await else {
            break;
        };
        if head
            .send(Item {
                page_index,
                page_id: *page_id,
                _permit: permit,
            })
            .await
            .is_err()
        {
            break;
        }
    }
    drop(head);

    // Every worker returns its thread to the pool once its channel is closed
    // and drained.
    for worker in workers {
        worker.join();
    }

    if cancel.load(Ordering::Relaxed) {
        bail!("cancelled");
    }

    if let Some(sink) = ctx.progress.as_ref() {
        sink(ProgressTick {
            step: None,
            step_id: String::new(),
            step_index: total_steps.saturating_sub(1),
            total_steps,
            page_index: total_pages.saturating_sub(1),
            total_pages,
            overall_percent: 100,
        });
    }
    Ok(RunOutcome {
        warning_count: tracker.warnings.load(Ordering::Relaxed),
    })
}

// ---------------------------------------------------------------------------
// Stage plumbing
// ---------------------------------------------------------------------------

/// A page travelling through the pipeline. The permit rides along so that
/// dropping an item anywhere — success, failure, or cancellation — releases
/// its in-flight slot without any explicit bookkeeping.
struct Item {
    page_index: usize,
    page_id: PageId,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

struct StageSpec {
    info: &'static EngineInfo,
    /// `None` when the engine failed to load; the stage then warns per page.
    engine: Option<Arc<dyn Engine>>,
    workers: usize,
    batch: usize,
}

impl StageSpec {
    fn clone_for_worker(&self) -> StageSpec {
        StageSpec {
            info: self.info,
            engine: self.engine.clone(),
            workers: self.workers,
            batch: self.batch,
        }
    }
}

/// Everything a stage worker needs that is shared across all stages.
struct StageContext {
    session: Arc<ProjectSession>,
    runtime: Arc<RuntimeManager>,
    llm: Arc<llm::Model>,
    renderer: Arc<renderer::Renderer>,
    cancel: Arc<AtomicBool>,
    options: PipelineRunOptions,
    progress: Option<ProgressSink>,
    warnings: Option<WarningSink>,
    tracker: Arc<Tracker>,
}

/// Progress bookkeeping shared by every worker.
struct Tracker {
    /// Steps completed-or-skipped per page, indexed by page index.
    steps_done: Vec<std::sync::atomic::AtomicUsize>,
    completed: std::sync::atomic::AtomicU64,
    warnings: std::sync::atomic::AtomicUsize,
    total_pages: usize,
    total_steps: usize,
    total_units: u64,
}

impl Tracker {
    fn new(total_pages: usize, total_steps: usize) -> Self {
        Self {
            steps_done: (0..total_pages)
                .map(|_| std::sync::atomic::AtomicUsize::new(0))
                .collect(),
            completed: std::sync::atomic::AtomicU64::new(0),
            warnings: std::sync::atomic::AtomicUsize::new(0),
            total_pages,
            total_steps,
            total_units: (total_pages * total_steps) as u64,
        }
    }

    fn credit(&self, page_index: usize, steps: usize) {
        if let Some(slot) = self.steps_done.get(page_index) {
            slot.fetch_add(steps, Ordering::Relaxed);
        }
        self.completed.fetch_add(steps as u64, Ordering::Relaxed);
    }

    /// Lowest page index not yet finished through every stage.
    ///
    /// Reported as `current_page` so the UI's "Image N/M" stays monotonic even
    /// though pages are in flight simultaneously — and so the frontend only
    /// treats a page as done once all of its ops really are in the scene.
    fn frontier(&self) -> usize {
        self.steps_done
            .iter()
            .position(|d| d.load(Ordering::Relaxed) < self.total_steps)
            .unwrap_or(self.total_pages.saturating_sub(1))
    }

    fn percent(&self) -> u8 {
        let done = self.completed.load(Ordering::Relaxed);
        ((done * 100) / self.total_units.max(1)).min(100) as u8
    }
}

/// A unit of stage work handed to a pooled thread, run on that thread's
/// long-lived runtime.
type StageJob = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send + 'static>;

/// Threads that run stage work and are never allowed to finish.
///
/// Each stage still gets a thread to itself for the length of a run: model
/// inference is synchronous and multi-second, so on a shared tokio worker pool
/// several concurrent stages would starve the HTTP server and the SSE stream. A
/// private thread also pins any GPU context to one thread, which is what
/// `comic-text-bubble-detector` already does for itself.
///
/// What the pool adds is that the thread *parks* when its stage ends instead of
/// exiting, because both of the things a stage thread owns are unsafe to tear
/// down:
///
/// * `candle` caches its cuDNN handles in a `thread_local!` (they are neither
///   `Send` nor `Sync`), and `cudarc`'s `Drop for Cudnn` unwraps `cudnnDestroy`.
///   A thread that ends therefore destroys those handles and turns any teardown
///   error into a panic inside a destructor — a hard crash, most easily hit by
///   cancelling a run, which ends every stage thread at once.
/// * Hyper's pooled connections belong to the runtime that opened them, and the
///   LLM client is one `Arc<ClientWithMiddleware>` shared by every stage. A
///   runtime that is dropped takes its connections with it, so a *different*
///   worker's next request fails with "error sending request".
///
/// Parking keeps the cuDNN handles and the CUDA context warm for the next run
/// as a side benefit.
struct StagePool {
    jobs: async_channel::Sender<StageJob>,
    /// Receiver kept alive here so the channel never closes and the threads
    /// never fall out of their loop.
    rx: async_channel::Receiver<StageJob>,
    /// Threads currently parked on `recv`, claimed by submitters.
    idle: Arc<AtomicUsize>,
}

static STAGE_POOL: OnceLock<StagePool> = OnceLock::new();

impl StagePool {
    fn get() -> &'static StagePool {
        STAGE_POOL.get_or_init(|| {
            let (jobs, rx) = async_channel::unbounded::<StageJob>();
            StagePool {
                jobs,
                rx,
                idle: Arc::new(AtomicUsize::new(0)),
            }
        })
    }

    /// Hand `job` to a parked thread, spawning a fresh one when none is free.
    ///
    /// The claim decrements before the job is queued so two stages can never
    /// count on the same parked thread. Over-spawning is harmless — the extra
    /// thread just parks — but under-spawning would deadlock a pipeline whose
    /// upstream stage is blocked on a bounded channel, so a failed claim always
    /// spawns.
    fn submit(&'static self, job: StageJob) {
        let claimed = self
            .idle
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
            .is_ok();
        if !claimed {
            self.spawn_thread();
        }
        // Unbounded, and the receiver lives in the pool, so this never blocks
        // and never fails.
        let _ = self.jobs.try_send(job);
    }

    fn spawn_thread(&'static self) {
        let rx = self.rx.clone();
        let idle = self.idle.clone();
        std::thread::spawn(move || {
            // Built on first use and then kept for the life of the thread.
            let mut rt: Option<tokio::runtime::Runtime> = None;
            // This thread was spawned to serve a job that is already queued, so
            // it is not idle on the way into the first `recv`.
            let mut park_counted = false;
            loop {
                if park_counted {
                    idle.fetch_add(1, Ordering::Release);
                }
                park_counted = true;
                let Ok(job) = rx.recv_blocking() else { return };

                if rt.is_none() {
                    match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(built) => rt = Some(built),
                        Err(err) => {
                            // Dropping the job releases its completion signal,
                            // so the driver stops waiting on a stage that will
                            // never run rather than hanging.
                            tracing::error!("stage runtime failed: {err:#}");
                            continue;
                        }
                    }
                }
                let rt = rt.as_ref().expect("runtime built above");

                // A panicking stage must not take the thread down with it: that
                // would destroy exactly the cuDNN state this pool exists to
                // keep alive. The driver sees the dropped signal and moves on,
                // which is what a panicked `JoinHandle` gave it before.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(rt)));
            }
        });
    }
}

/// Waits for one stage worker to finish. The pooled thread outlives the run, so
/// completion is signalled by the job dropping its sender — on panic too —
/// rather than by joining a thread.
struct StageHandle(std::sync::mpsc::Receiver<()>);

impl StageHandle {
    fn join(self) {
        // `Err` means the worker panicked or was dropped before signalling,
        // which counts as finished just as `JoinHandle::join` did.
        let _ = self.0.recv();
    }
}

/// Run one stage on a pooled thread with its own current-thread runtime.
fn spawn_stage_worker(
    ctx: Arc<StageContext>,
    stage: StageSpec,
    seq: usize,
    rx: async_channel::Receiver<Item>,
    forward: Option<async_channel::Sender<Item>>,
) -> StageHandle {
    let (done, wait) = std::sync::mpsc::channel::<()>();
    StagePool::get().submit(Box::new(move |rt| {
        let _done = done;
        rt.block_on(stage_loop(ctx, stage, seq, rx, forward));
    }));
    StageHandle(wait)
}

/// Take one page, then greedily add whatever is *already* sitting in the
/// channel, up to `max`.
///
/// Never waits for a batch to fill. An idle queue yields a batch of one, so a
/// pipeline that isn't backed up behaves exactly like the sequential driver
/// and adds no latency; batching only kicks in where work has actually piled
/// up. `None` means the channel closed and the stage is finished.
async fn drain_batch<T>(rx: &async_channel::Receiver<T>, max: usize) -> Option<Vec<T>> {
    let first = rx.recv().await.ok()?;
    let mut items = vec![first];
    while items.len() < max {
        match rx.try_recv() {
            Ok(item) => items.push(item),
            Err(_) => break,
        }
    }
    Some(items)
}

async fn stage_loop(
    ctx: Arc<StageContext>,
    stage: StageSpec,
    seq: usize,
    rx: async_channel::Receiver<Item>,
    forward: Option<async_channel::Sender<Item>>,
) {
    let tracker = &ctx.tracker;
    let remaining_steps = tracker.total_steps - seq;

    while let Some(items) = drain_batch(&rx, stage.batch).await {
        if ctx.cancel.load(Ordering::Relaxed) {
            return;
        }

        // One tick per batch, not per page: the tick is derived from the
        // tracker, so repeating it for each page in a batch would put
        // identical frames on the SSE bus.
        if let Some(sink) = ctx.progress.as_ref() {
            sink(ProgressTick {
                step: step_for(stage.info),
                step_id: stage.info.id.to_string(),
                step_index: seq,
                total_steps: tracker.total_steps,
                page_index: tracker.frontier(),
                total_pages: tracker.total_pages,
                overall_percent: tracker.percent(),
            });
        }
        // Give this thread's runtime a chance to flush the frame before the
        // next long, fully synchronous inference call monopolises it.
        tokio::task::yield_now().await;

        // Engine never loaded: warn once per page, same as the sequential
        // driver did when a lazy load failed.
        let Some(engine) = stage.engine.as_ref() else {
            for item in items {
                report_step_failure(
                    stage.info.id,
                    &item.page_id,
                    seq,
                    item.page_index,
                    tracker.total_pages,
                    tracker.total_steps,
                    &anyhow::anyhow!("engine failed to load"),
                    &ctx.tracker.warnings,
                    ctx.warnings.as_ref(),
                );
                tracker.credit(item.page_index, remaining_steps);
            }
            continue;
        };

        // Drop pages the user deleted mid-run before touching the engine.
        let scene_snap = ctx.session.scene_snapshot();
        let mut live = Vec::with_capacity(items.len());
        for item in items {
            if scene_snap.pages.contains_key(&item.page_id) {
                live.push(item);
            } else {
                tracker.credit(item.page_index, remaining_steps);
            }
        }
        if live.is_empty() {
            continue;
        }

        let ctxs: Vec<EngineCtx<'_>> = live
            .iter()
            .map(|item| EngineCtx {
                scene: &scene_snap,
                page: item.page_id,
                blobs: &ctx.session.blobs,
                runtime: &ctx.runtime,
                cancel: &ctx.cancel,
                options: &ctx.options,
                llm: &ctx.llm,
                renderer: &ctx.renderer,
            })
            .collect();

        let results = async { engine.run_batch(ctxs).await }
            .instrument(tracing::info_span!(
                "step",
                engine = stage.info.id,
                pages = live.len()
            ))
            .await;

        for (item, result) in live.into_iter().zip(results) {
            match result {
                Ok(ops) => {
                    if !ops.is_empty() {
                        let batch = Op::Batch {
                            ops,
                            label: format!("{}: page {}", stage.info.id, item.page_id),
                        };
                        if let Err(err) = ctx.session.apply(batch) {
                            report_step_failure(
                                stage.info.id,
                                &item.page_id,
                                seq,
                                item.page_index,
                                tracker.total_pages,
                                tracker.total_steps,
                                &err,
                                &ctx.tracker.warnings,
                                ctx.warnings.as_ref(),
                            );
                            tracker.credit(item.page_index, remaining_steps);
                            continue;
                        }
                    }
                    tracker.credit(item.page_index, 1);
                    // Not forwarding is how a page stops: the last stage has
                    // no downstream, and a dropped item frees its permit.
                    if let Some(tx) = forward.as_ref()
                        && tx.send(item).await.is_err()
                    {
                        return;
                    }
                }
                Err(err) => {
                    report_step_failure(
                        stage.info.id,
                        &item.page_id,
                        seq,
                        item.page_index,
                        tracker.total_pages,
                        tracker.total_steps,
                        &err,
                        &ctx.tracker.warnings,
                        ctx.warnings.as_ref(),
                    );
                    // Later steps almost always consume this step's artifact,
                    // so the page stops here and its remaining steps are
                    // credited to keep progress reaching 100%.
                    tracker.credit(item.page_index, remaining_steps);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn report_step_failure(
    engine_id: &str,
    page_id: &PageId,
    step_index: usize,
    page_index: usize,
    total_pages: usize,
    total_steps: usize,
    err: &anyhow::Error,
    warning_count: &std::sync::atomic::AtomicUsize,
    sink: Option<&WarningSink>,
) {
    let _ = total_steps;
    tracing::warn!(
        engine = engine_id,
        page = %page_id,
        step_index,
        "pipeline step failed: {err:#}"
    );
    warning_count.fetch_add(1, Ordering::Relaxed);
    if let Some(sink) = sink {
        sink(WarningTick {
            step_id: engine_id.to_string(),
            page_index,
            total_pages,
            message: format!("{err:#}"),
        });
    }
}

// ---------------------------------------------------------------------------
// Engine catalog building (API surface)
// ---------------------------------------------------------------------------

use koharu_core::{EngineCatalog, EngineCatalogEntry};

/// Build the engine catalog DTO for the API.
pub fn catalog() -> EngineCatalog {
    let entry = |info: &&EngineInfo| EngineCatalogEntry {
        id: info.id.to_string(),
        name: info.name.to_string(),
        produces: info.produces.iter().map(|a| format!("{a:?}")).collect(),
    };
    EngineCatalog {
        detectors: Registry::providers(Artifact::TextBoxes)
            .iter()
            .map(entry)
            .collect(),
        font_detectors: Registry::providers(Artifact::FontPredictions)
            .iter()
            .map(entry)
            .collect(),
        segmenters: Registry::providers(Artifact::SegmentMask)
            .iter()
            .map(entry)
            .collect(),
        bubble_segmenters: Registry::providers(Artifact::BubbleMask)
            .iter()
            .map(entry)
            .collect(),
        ocr: Registry::providers(Artifact::OcrText)
            .iter()
            .map(entry)
            .collect(),
        translators: Registry::providers(Artifact::Translations)
            .iter()
            .map(entry)
            .collect(),
        inpainters: Registry::providers(Artifact::Inpainted)
            .iter()
            .map(entry)
            .collect(),
        renderers: Registry::providers(Artifact::FinalRender)
            .iter()
            .map(entry)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_includes_anime_text_detector() {
        let catalog = catalog();

        assert!(catalog.detectors.iter().any(|engine| {
            engine.id == "anime-text"
                && engine.name == "Anime Text YOLO (N)"
                && engine.produces.iter().map(String::as_str).eq(["TextBoxes"])
        }));
    }

    // --- batching policy: take what's queued, never wait -------------------

    #[tokio::test]
    async fn idle_queue_yields_a_batch_of_one() {
        let (tx, rx) = async_channel::unbounded::<u8>();
        tx.send(1).await.unwrap();
        // Four more are allowed, but nothing else is queued, so the stage must
        // not block waiting for them.
        assert_eq!(drain_batch(&rx, 4).await, Some(vec![1]));
    }

    #[tokio::test]
    async fn backed_up_queue_is_drained_up_to_the_cap() {
        let (tx, rx) = async_channel::unbounded::<u8>();
        for i in 1..=6 {
            tx.send(i).await.unwrap();
        }
        assert_eq!(drain_batch(&rx, 4).await, Some(vec![1, 2, 3, 4]));
        // The remainder stays queued for the next pass.
        assert_eq!(drain_batch(&rx, 4).await, Some(vec![5, 6]));
    }

    #[tokio::test]
    async fn batch_of_one_never_groups() {
        let (tx, rx) = async_channel::unbounded::<u8>();
        for i in 1..=3 {
            tx.send(i).await.unwrap();
        }
        // `max_batch_pages = 1` / a non-batching engine must stay one-at-a-time
        // even when the queue is full.
        assert_eq!(drain_batch(&rx, 1).await, Some(vec![1]));
        assert_eq!(drain_batch(&rx, 1).await, Some(vec![2]));
    }

    #[tokio::test]
    async fn closed_and_drained_channel_ends_the_stage() {
        let (tx, rx) = async_channel::unbounded::<u8>();
        tx.send(1).await.unwrap();
        drop(tx);
        // Items already queued are still delivered after close.
        assert_eq!(drain_batch(&rx, 4).await, Some(vec![1]));
        assert_eq!(drain_batch(&rx, 4).await, None);
    }

    // --- progress accounting ---------------------------------------------

    #[test]
    fn frontier_tracks_the_lowest_unfinished_page() {
        let t = Tracker::new(3, 2);
        assert_eq!(t.frontier(), 0);

        // Page 1 finishing first must not advance the frontier past page 0 —
        // this is what keeps the UI's "Image N/M" monotonic while pages run
        // out of order.
        t.credit(1, 2);
        assert_eq!(t.frontier(), 0);

        t.credit(0, 1);
        assert_eq!(t.frontier(), 0);
        t.credit(0, 1);
        assert_eq!(t.frontier(), 2);
    }

    #[test]
    fn frontier_never_decreases_under_interleaved_completion() {
        let t = Tracker::new(4, 3);
        let mut last = t.frontier();
        // Deliberately out-of-order completions.
        for (page, steps) in [(2, 3), (0, 1), (3, 3), (0, 2), (1, 3)] {
            t.credit(page, steps);
            let now = t.frontier();
            assert!(now >= last, "frontier went backwards: {last} -> {now}");
            last = now;
        }
    }

    #[test]
    fn percent_reaches_100_when_every_step_is_credited() {
        let t = Tracker::new(2, 3);
        assert_eq!(t.percent(), 0);
        t.credit(0, 3);
        assert_eq!(t.percent(), 50);
        // A page that fails early still credits its remaining steps, so a run
        // with failures reaches 100 rather than stalling short.
        t.credit(1, 3);
        assert_eq!(t.percent(), 100);
    }

    #[test]
    fn percent_is_clamped_if_over_credited() {
        let t = Tracker::new(1, 1);
        t.credit(0, 5);
        assert_eq!(t.percent(), 100);
    }

    // --- stage pool: threads must outlive the stages they run --------------

    /// One global pool, so these tests must not run against each other.
    static POOL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    thread_local! {
        /// Stands in for candle's cuDNN handle: state that exists only as long
        /// as the thread holding it does.
        static STAGE_LOCAL_MARK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Submit one job and wait for its result.
    fn run_on_pool<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        StagePool::get().submit(Box::new(move |_rt| {
            let _ = tx.send(job());
        }));
        rx.recv().expect("pooled job never reported")
    }

    /// A job signals completion when its body ends, which is a moment before
    /// its thread re-parks. Waiting for the park means the next submit claims
    /// that thread rather than spawning another.
    fn wait_for_parked_thread() {
        for _ in 0..1_000 {
            if StagePool::get().idle.load(Ordering::Acquire) > 0 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// The whole point of the pool: a finished stage parks its thread instead
    /// of ending it, so candle's `thread_local!` cuDNN handles are never
    /// destroyed by an unwrapping `Drop` and hyper's pooled connections keep
    /// the runtime they were opened on.
    #[test]
    fn pooled_thread_keeps_thread_locals_across_stages() {
        let _guard = POOL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        wait_for_parked_thread();
        let marked = run_on_pool(|| {
            STAGE_LOCAL_MARK.with(|m| m.set(true));
            std::thread::current().id()
        });

        // Land on that thread again. Had it ended with its stage, the mark
        // would have gone with it.
        for _ in 0..200 {
            wait_for_parked_thread();
            let (id, still_marked) = run_on_pool(|| {
                (
                    std::thread::current().id(),
                    STAGE_LOCAL_MARK.with(|m| m.get()),
                )
            });
            if id == marked {
                assert!(still_marked, "pooled thread lost its thread-local state");
                return;
            }
        }
        panic!("stage thread {marked:?} never took another job; it did not survive its stage");
    }

    /// A panicking stage must not take its thread — and so that thread's CUDA
    /// state — down with it.
    #[test]
    fn panicking_stage_does_not_kill_its_pooled_thread() {
        let _guard = POOL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let (done, wait) = std::sync::mpsc::channel::<()>();
        StagePool::get().submit(Box::new(move |_rt| {
            let _done = done;
            panic!("stage blew up");
        }));
        // The dropped sender releases the driver whether the stage returned or
        // unwound, which is what a panicked `JoinHandle` gave it before.
        assert!(
            wait.recv().is_err(),
            "panicking stage should not signal success"
        );

        // The pool still serves work.
        assert_eq!(run_on_pool(|| 7), 7);
    }

    /// Concurrent stages must never be handed the same parked thread: an
    /// upstream stage blocked on a bounded channel would deadlock waiting on a
    /// downstream stage that has nowhere to run.
    #[test]
    fn concurrent_stages_each_get_their_own_thread() {
        let _guard = POOL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let barrier = Arc::new(std::sync::Barrier::new(4));
        let mut waits = Vec::new();

        for _ in 0..4 {
            let barrier = barrier.clone();
            let (done, wait) = std::sync::mpsc::channel::<()>();
            StagePool::get().submit(Box::new(move |_rt| {
                let _done = done;
                // Only completes if all four are running at once.
                barrier.wait();
            }));
            waits.push(wait);
        }

        for wait in waits {
            let _ = wait.recv();
        }
    }
}
