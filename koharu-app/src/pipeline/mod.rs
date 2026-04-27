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
    BoxFuture, Engine, EngineCtx, EngineInfo, EngineLoadFn, PipelineRunOptions, Registry,
    build_order,
};

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use koharu_core::{NodeDataPatch, NodeId, NodePatch, Op, PageId, PipelineStep, TextDataPatch};
use koharu_runtime::RuntimeManager;
use tokio::task::JoinHandle;
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
    /// Engine id (e.g. `"paddle-ocr-vl-1.5"`) for diagnostics + logs.
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
use crate::terminology::{
    PlaceholderReplacement, protect_text, restore_text, system_prompt_with_placeholders,
};

// ---------------------------------------------------------------------------
// Spec + scope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PipelineSpec {
    pub scope: Scope,
    pub steps: Vec<String>,
    pub options: PipelineRunOptions,
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

    if let Some(limit) = spec.options.batch_translation_char_limit
        && infos
            .iter()
            .any(|info| info.produces.contains(&Artifact::Translations))
    {
        return run_with_batch_translation(
            session, registry, runtime, cpu, llm, renderer, spec, cancel, progress, warnings,
            infos, order, limit,
        )
        .await;
    }

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
    let total_units = (total_pages * total_steps) as u64;
    let mut completed: u64 = 0;
    let mut warning_count: usize = 0;

    'pages: for (page_index, page_id) in pages.iter().enumerate() {
        for (seq, &i) in order.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            let info = infos[i];

            if let Some(sink) = progress.as_ref() {
                let percent = ((completed * 100) / total_units).min(100) as u8;
                sink(ProgressTick {
                    step: step_for(info),
                    step_id: info.id.to_string(),
                    step_index: seq,
                    total_steps,
                    page_index,
                    total_pages,
                    overall_percent: percent,
                });
            }

            // The page must still exist (user may have deleted it mid-run).
            if !session.scene.read().pages.contains_key(page_id) {
                // Skip the remaining steps for a deleted page and credit all
                // of them against total_units so progress still reaches 100%.
                completed += (total_steps - seq) as u64;
                continue 'pages;
            }

            let engine = match registry.get(info.id, &runtime, cpu).await {
                Ok(e) => e,
                Err(err) => {
                    // Engine *load* failure: same recovery as a run failure.
                    report_step_failure(
                        info.id,
                        page_id,
                        seq,
                        page_index,
                        total_pages,
                        total_steps,
                        &err,
                        &mut warning_count,
                        warnings.as_ref(),
                    );
                    completed += (total_steps - seq) as u64;
                    continue 'pages;
                }
            };
            let scene_snap = session.scene_snapshot();
            let ctx = EngineCtx {
                scene: &scene_snap,
                page: *page_id,
                blobs: &session.blobs,
                runtime: &runtime,
                cancel: &cancel,
                options: &spec.options,
                llm: &llm,
                renderer: &renderer,
            };
            let step_result = async { engine.run(ctx).await }
                .instrument(tracing::info_span!("step", engine = info.id, page = %page_id))
                .await;
            let ops = match step_result {
                Ok(ops) => ops,
                Err(err) => {
                    report_step_failure(
                        info.id,
                        page_id,
                        seq,
                        page_index,
                        total_pages,
                        total_steps,
                        &err,
                        &mut warning_count,
                        warnings.as_ref(),
                    );
                    // Subsequent steps on this page almost always consume the
                    // failed step's artifact; skip the rest and move on.
                    completed += (total_steps - seq) as u64;
                    continue 'pages;
                }
            };
            completed += 1;
            if ops.is_empty() {
                continue;
            }
            let batch = Op::Batch {
                ops,
                label: format!("{}: page {}", info.id, page_id),
            };
            if let Err(err) = session.apply(batch) {
                report_step_failure(
                    info.id,
                    page_id,
                    seq,
                    page_index,
                    total_pages,
                    total_steps,
                    &err,
                    &mut warning_count,
                    warnings.as_ref(),
                );
                continue 'pages;
            }
        }
    }

    if let Some(sink) = progress.as_ref() {
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
    Ok(RunOutcome { warning_count })
}

#[allow(clippy::too_many_arguments)]
async fn run_with_batch_translation(
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
    infos: Vec<&'static EngineInfo>,
    order: Vec<usize>,
    batch_char_limit: usize,
) -> Result<RunOutcome> {
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

    let translation_steps = order
        .iter()
        .copied()
        .filter(|&i| infos[i].produces.contains(&Artifact::Translations))
        .collect::<Vec<_>>();
    if translation_steps.len() != 1 {
        bail!("batch translation requires exactly one translation engine");
    }

    let pre_steps = order
        .iter()
        .copied()
        .filter(|&i| {
            !infos[i].produces.contains(&Artifact::Translations)
                && matches!(
                    step_for(infos[i]),
                    Some(PipelineStep::Detect | PipelineStep::Ocr)
                )
        })
        .collect::<Vec<_>>();
    let post_steps = order
        .iter()
        .copied()
        .filter(|&i| !pre_steps.contains(&i) && !translation_steps.contains(&i))
        .collect::<Vec<_>>();
    let render_steps = post_steps
        .iter()
        .copied()
        .filter(|&i| infos[i].produces.contains(&Artifact::FinalRender))
        .collect::<Vec<_>>();
    let non_render_post_steps = post_steps
        .iter()
        .copied()
        .filter(|i| !render_steps.contains(i))
        .collect::<Vec<_>>();

    let total_pages = pages.len().max(1);
    let total_steps = pre_steps.len() + non_render_post_steps.len() + render_steps.len() + 1;
    let total_units = (total_pages * total_steps).max(1);
    let mut completed = 0usize;
    let mut warning_count = 0usize;
    let mut last_percent = 0u8;
    let mut states = BatchPageStates::new(&pages);
    let mut pending_targets = Vec::new();
    let mut translation_task: Option<TranslationHandle> = None;
    let mut next_batch_index = 0usize;
    let translation_info = infos[translation_steps[0]];

    for (page_index, page_id) in pages.iter().enumerate() {
        for (seq, &i) in pre_steps.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                if let Some(task) = translation_task.take() {
                    task.abort();
                }
                bail!("cancelled");
            }
            let info = infos[i];
            emit_progress(
                progress.as_ref(),
                info,
                seq,
                total_steps,
                page_index,
                total_pages,
                completed,
                total_units,
                &mut last_percent,
            );

            if !session.scene.read().pages.contains_key(page_id) {
                states.mark_skipped(*page_id);
                break;
            }

            let ok = run_one_step(
                &session,
                &registry,
                &runtime,
                cpu,
                &llm,
                &renderer,
                &spec.options,
                &cancel,
                info,
                *page_id,
                seq,
                page_index,
                total_pages,
                total_steps,
                &mut warning_count,
                warnings.as_ref(),
            )
            .await?;
            completed += 1;
            if !ok {
                states.mark_skipped(*page_id);
                break;
            }

            if let Some(task) = translation_task.as_ref() {
                if task.is_finished() {
                    let task = translation_task.take().expect("translation task exists");
                    finish_translation_batch(task, &session, &mut states, translation_info).await?;
                    completed += 1;
                }
            }
        }

        if states.is_skipped(*page_id) {
            continue;
        }

        let scene_snap = session.scene_snapshot();
        let targets = collect_translation_targets(&scene_snap, *page_id, &spec.options);
        states.mark_pre_done(*page_id, targets.len());
        pending_targets.extend(targets);

        if translation_task.is_none() && queued_char_count(&pending_targets) >= batch_char_limit {
            let batch = take_next_translation_batch(&mut pending_targets, batch_char_limit);
            emit_progress(
                progress.as_ref(),
                translation_info,
                pre_steps.len(),
                total_steps,
                page_index,
                total_pages,
                completed,
                total_units,
                &mut last_percent,
            );
            translation_task = Some(spawn_translation_batch(
                Arc::clone(&llm),
                batch,
                spec.options.target_language.clone(),
                batch_translation_system_prompt(&spec.options),
                next_batch_index,
            ));
            next_batch_index += 1;
        }
    }

    if translation_task.is_none() && !pending_targets.is_empty() {
        translation_task = Some(spawn_translation_batch(
            Arc::clone(&llm),
            take_next_translation_batch(&mut pending_targets, batch_char_limit),
            spec.options.target_language.clone(),
            batch_translation_system_prompt(&spec.options),
            next_batch_index,
        ));
        next_batch_index += 1;
    }

    if non_render_post_steps.is_empty() {
        for page_id in &pages {
            if states.pre_done(*page_id) && !states.is_skipped(*page_id) {
                states.mark_inpaint_done(*page_id);
            }
        }
    }

    for (page_index, page_id) in pages.iter().enumerate() {
        if !states.ready_for_inpaint(*page_id) {
            continue;
        }
        for (seq, &i) in non_render_post_steps.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                if let Some(task) = translation_task.take() {
                    task.abort();
                }
                bail!("cancelled");
            }
            let info = infos[i];
            emit_progress(
                progress.as_ref(),
                info,
                pre_steps.len() + 1 + seq,
                total_steps,
                page_index,
                total_pages,
                completed,
                total_units,
                &mut last_percent,
            );
            if !session.scene.read().pages.contains_key(page_id) {
                states.mark_skipped(*page_id);
                break;
            }
            let ok = run_one_step(
                &session,
                &registry,
                &runtime,
                cpu,
                &llm,
                &renderer,
                &spec.options,
                &cancel,
                info,
                *page_id,
                pre_steps.len() + 1 + seq,
                page_index,
                total_pages,
                total_steps,
                &mut warning_count,
                warnings.as_ref(),
            )
            .await?;
            completed += 1;
            if !ok {
                states.mark_skipped(*page_id);
                break;
            }

            if let Some(task) = translation_task.as_ref() {
                if task.is_finished() {
                    let task = translation_task.take().expect("translation task exists");
                    finish_translation_batch(task, &session, &mut states, translation_info).await?;
                    completed += 1;
                    if translation_task.is_none() && !pending_targets.is_empty() {
                        translation_task = Some(spawn_translation_batch(
                            Arc::clone(&llm),
                            take_next_translation_batch(&mut pending_targets, batch_char_limit),
                            spec.options.target_language.clone(),
                            batch_translation_system_prompt(&spec.options),
                            next_batch_index,
                        ));
                        next_batch_index += 1;
                    }
                }
            }

            run_ready_renders(
                &pages,
                &mut states,
                &render_steps,
                &session,
                &registry,
                &runtime,
                cpu,
                &llm,
                &renderer,
                &spec.options,
                &cancel,
                &progress,
                &warnings,
                infos.as_slice(),
                pre_steps.len() + 1 + non_render_post_steps.len(),
                total_steps,
                total_pages,
                total_units,
                &mut completed,
                &mut warning_count,
                &mut last_percent,
            )
            .await?;
        }
        if !states.is_skipped(*page_id) {
            states.mark_inpaint_done(*page_id);
        }
        run_ready_renders(
            &pages,
            &mut states,
            &render_steps,
            &session,
            &registry,
            &runtime,
            cpu,
            &llm,
            &renderer,
            &spec.options,
            &cancel,
            &progress,
            &warnings,
            infos.as_slice(),
            pre_steps.len() + 1 + non_render_post_steps.len(),
            total_steps,
            total_pages,
            total_units,
            &mut completed,
            &mut warning_count,
            &mut last_percent,
        )
        .await?;
    }

    while translation_task.is_some() || !pending_targets.is_empty() {
        if cancel.load(Ordering::Relaxed) {
            if let Some(task) = translation_task.take() {
                task.abort();
            }
            bail!("cancelled");
        }
        if translation_task.is_none() && !pending_targets.is_empty() {
            translation_task = Some(spawn_translation_batch(
                Arc::clone(&llm),
                take_next_translation_batch(&mut pending_targets, batch_char_limit),
                spec.options.target_language.clone(),
                batch_translation_system_prompt(&spec.options),
                next_batch_index,
            ));
            next_batch_index += 1;
        }
        if let Some(task) = translation_task.take() {
            emit_progress(
                progress.as_ref(),
                translation_info,
                pre_steps.len(),
                total_steps,
                total_pages.saturating_sub(1),
                total_pages,
                completed,
                total_units,
                &mut last_percent,
            );
            finish_translation_batch(task, &session, &mut states, translation_info).await?;
            completed += 1;
        }
        run_ready_renders(
            &pages,
            &mut states,
            &render_steps,
            &session,
            &registry,
            &runtime,
            cpu,
            &llm,
            &renderer,
            &spec.options,
            &cancel,
            &progress,
            &warnings,
            infos.as_slice(),
            pre_steps.len() + 1 + non_render_post_steps.len(),
            total_steps,
            total_pages,
            total_units,
            &mut completed,
            &mut warning_count,
            &mut last_percent,
        )
        .await?;
    }

    run_ready_renders(
        &pages,
        &mut states,
        &render_steps,
        &session,
        &registry,
        &runtime,
        cpu,
        &llm,
        &renderer,
        &spec.options,
        &cancel,
        &progress,
        &warnings,
        infos.as_slice(),
        pre_steps.len() + 1 + non_render_post_steps.len(),
        total_steps,
        total_pages,
        total_units,
        &mut completed,
        &mut warning_count,
        &mut last_percent,
    )
    .await?;

    if let Some(sink) = progress.as_ref() {
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

    Ok(RunOutcome { warning_count })
}

#[allow(clippy::too_many_arguments)]
async fn run_ready_renders(
    pages: &[PageId],
    states: &mut BatchPageStates,
    render_steps: &[usize],
    session: &Arc<ProjectSession>,
    registry: &Arc<Registry>,
    runtime: &Arc<RuntimeManager>,
    cpu: bool,
    llm: &Arc<llm::Model>,
    renderer: &Arc<renderer::Renderer>,
    options: &PipelineRunOptions,
    cancel: &Arc<AtomicBool>,
    progress: &Option<ProgressSink>,
    warnings: &Option<WarningSink>,
    infos: &[&'static EngineInfo],
    step_index_base: usize,
    total_steps: usize,
    total_pages: usize,
    total_units: usize,
    completed: &mut usize,
    warning_count: &mut usize,
    last_percent: &mut u8,
) -> Result<()> {
    for (page_index, page_id) in pages.iter().enumerate() {
        if !states.ready_for_render(*page_id) {
            continue;
        }
        if render_steps.is_empty() {
            states.mark_render_done(*page_id);
            continue;
        }

        let mut page_ok = true;
        for (seq, &i) in render_steps.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            let info = infos[i];
            emit_progress(
                progress.as_ref(),
                info,
                step_index_base + seq,
                total_steps,
                page_index,
                total_pages,
                *completed,
                total_units,
                last_percent,
            );
            if !session.scene.read().pages.contains_key(page_id) {
                states.mark_skipped(*page_id);
                page_ok = false;
                break;
            }
            let ok = run_one_step(
                session,
                registry,
                runtime,
                cpu,
                llm,
                renderer,
                options,
                cancel,
                info,
                *page_id,
                step_index_base + seq,
                page_index,
                total_pages,
                total_steps,
                warning_count,
                warnings.as_ref(),
            )
            .await?;
            *completed += 1;
            if !ok {
                states.mark_skipped(*page_id);
                page_ok = false;
                break;
            }
        }

        if page_ok {
            states.mark_render_done(*page_id);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_one_step(
    session: &Arc<ProjectSession>,
    registry: &Arc<Registry>,
    runtime: &Arc<RuntimeManager>,
    cpu: bool,
    llm: &Arc<llm::Model>,
    renderer: &Arc<renderer::Renderer>,
    options: &PipelineRunOptions,
    cancel: &Arc<AtomicBool>,
    info: &EngineInfo,
    page_id: PageId,
    step_index: usize,
    page_index: usize,
    total_pages: usize,
    total_steps: usize,
    warning_count: &mut usize,
    warnings: Option<&WarningSink>,
) -> Result<bool> {
    let engine = match registry.get(info.id, runtime, cpu).await {
        Ok(e) => e,
        Err(err) => {
            report_step_failure(
                info.id,
                &page_id,
                step_index,
                page_index,
                total_pages,
                total_steps,
                &err,
                warning_count,
                warnings,
            );
            return Ok(false);
        }
    };
    let scene_snap = session.scene_snapshot();
    let ctx = EngineCtx {
        scene: &scene_snap,
        page: page_id,
        blobs: &session.blobs,
        runtime: runtime.as_ref(),
        cancel: cancel.as_ref(),
        options,
        llm: llm.as_ref(),
        renderer: renderer.as_ref(),
    };
    let step_result = async { engine.run(ctx).await }
        .instrument(tracing::info_span!("step", engine = info.id, page = %page_id))
        .await;
    let ops = match step_result {
        Ok(ops) => ops,
        Err(err) => {
            report_step_failure(
                info.id,
                &page_id,
                step_index,
                page_index,
                total_pages,
                total_steps,
                &err,
                warning_count,
                warnings,
            );
            return Ok(false);
        }
    };
    if ops.is_empty() {
        return Ok(true);
    }
    let batch = Op::Batch {
        ops,
        label: format!("{}: page {}", info.id, page_id),
    };
    if let Err(err) = session.apply(batch) {
        report_step_failure(
            info.id,
            &page_id,
            step_index,
            page_index,
            total_pages,
            total_steps,
            &err,
            warning_count,
            warnings,
        );
        return Ok(false);
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn emit_progress(
    sink: Option<&ProgressSink>,
    info: &EngineInfo,
    step_index: usize,
    total_steps: usize,
    page_index: usize,
    total_pages: usize,
    completed: usize,
    total_units: usize,
    last_percent: &mut u8,
) {
    if let Some(sink) = sink {
        let percent = (((completed as u64) * 100) / (total_units as u64)).min(100) as u8;
        *last_percent = (*last_percent).max(percent);
        sink(ProgressTick {
            step: step_for(info),
            step_id: info.id.to_string(),
            step_index,
            total_steps,
            page_index,
            total_pages,
            overall_percent: *last_percent,
        });
    }
}

#[derive(Debug, Clone)]
struct TranslationTarget {
    page_id: PageId,
    node_id: NodeId,
    source: String,
    replacements: Vec<PlaceholderReplacement>,
}

#[derive(Debug, Clone, Default)]
struct BatchPageState {
    skipped: bool,
    pre_done: bool,
    pending_translations: usize,
    translated: bool,
    inpaint_done: bool,
    render_done: bool,
}

#[derive(Debug, Clone)]
struct BatchPageStates {
    pages: HashMap<PageId, BatchPageState>,
}

impl BatchPageStates {
    fn new(pages: &[PageId]) -> Self {
        Self {
            pages: pages
                .iter()
                .copied()
                .map(|page_id| (page_id, BatchPageState::default()))
                .collect(),
        }
    }

    fn state_mut(&mut self, page_id: PageId) -> &mut BatchPageState {
        self.pages.entry(page_id).or_default()
    }

    fn pre_done(&self, page_id: PageId) -> bool {
        self.pages.get(&page_id).is_some_and(|state| state.pre_done)
    }

    fn is_skipped(&self, page_id: PageId) -> bool {
        self.pages.get(&page_id).map_or(true, |state| state.skipped)
    }

    fn mark_skipped(&mut self, page_id: PageId) {
        self.state_mut(page_id).skipped = true;
    }

    fn mark_pre_done(&mut self, page_id: PageId, translation_count: usize) {
        let state = self.state_mut(page_id);
        state.pre_done = true;
        state.pending_translations = translation_count;
        state.translated = translation_count == 0;
    }

    fn mark_translated(&mut self, page_id: PageId, count: usize) {
        let state = self.state_mut(page_id);
        state.pending_translations = state.pending_translations.saturating_sub(count);
        state.translated = state.pending_translations == 0;
    }

    fn mark_inpaint_done(&mut self, page_id: PageId) {
        self.state_mut(page_id).inpaint_done = true;
    }

    fn mark_render_done(&mut self, page_id: PageId) {
        self.state_mut(page_id).render_done = true;
    }

    fn ready_for_inpaint(&self, page_id: PageId) -> bool {
        self.pages
            .get(&page_id)
            .is_some_and(|state| state.pre_done && !state.inpaint_done && !state.skipped)
    }

    fn ready_for_render(&self, page_id: PageId) -> bool {
        self.pages.get(&page_id).is_some_and(|state| {
            state.pre_done
                && state.translated
                && state.inpaint_done
                && !state.render_done
                && !state.skipped
        })
    }
}

fn collect_translation_targets(
    scene: &koharu_core::Scene,
    page_id: PageId,
    options: &PipelineRunOptions,
) -> Vec<TranslationTarget> {
    let Some(page) = scene.pages.get(&page_id) else {
        return Vec::new();
    };
    page.nodes
        .iter()
        .filter_map(|(node_id, node)| {
            let koharu_core::NodeKind::Text(text_data) = &node.kind else {
                return None;
            };
            let source = text_data.text.as_ref()?;
            if source.trim().is_empty() {
                return None;
            }
            let protected = protect_text(source, &options.terminology);
            Some(TranslationTarget {
                page_id,
                node_id: *node_id,
                source: protected.text,
                replacements: protected.replacements,
            })
        })
        .collect()
}

fn batch_translation_system_prompt(options: &PipelineRunOptions) -> Option<String> {
    system_prompt_with_placeholders(
        options.system_prompt.as_deref(),
        options.target_language.as_deref(),
        !options.terminology.is_empty(),
    )
}

struct CompletedTranslationBatch {
    index: usize,
    targets: Vec<TranslationTarget>,
    translations: Vec<String>,
}

type TranslationHandle = JoinHandle<Result<CompletedTranslationBatch>>;

fn queued_char_count(targets: &[TranslationTarget]) -> usize {
    targets
        .iter()
        .map(|target| target.source.chars().count())
        .sum()
}

fn take_next_translation_batch(
    pending_targets: &mut Vec<TranslationTarget>,
    char_limit: usize,
) -> Vec<TranslationTarget> {
    let char_limit = char_limit.max(1);
    let mut take_count = 0usize;
    let mut chars = 0usize;
    for target in pending_targets.iter() {
        let target_chars = target.source.chars().count();
        if take_count > 0 && chars + target_chars > char_limit {
            break;
        }
        chars += target_chars;
        take_count += 1;
    }
    pending_targets.drain(..take_count).collect()
}

fn spawn_translation_batch(
    llm: Arc<llm::Model>,
    targets: Vec<TranslationTarget>,
    target_language: Option<String>,
    system_prompt: Option<String>,
    index: usize,
) -> TranslationHandle {
    tokio::spawn(async move {
        let sources = targets
            .iter()
            .map(|target| target.source.clone())
            .collect::<Vec<_>>();
        let translations = llm
            .translate_xml_tagged_texts(
                &sources,
                target_language.as_deref(),
                system_prompt.as_deref(),
            )
            .await?;
        Ok(CompletedTranslationBatch {
            index,
            targets,
            translations,
        })
    })
}

async fn finish_translation_batch(
    handle: TranslationHandle,
    session: &Arc<ProjectSession>,
    states: &mut BatchPageStates,
    info: &EngineInfo,
) -> Result<()> {
    let completed = handle.await.context("translation task panicked")??;
    for target in &completed.targets {
        states.mark_translated(target.page_id, 1);
    }
    let ops = translation_ops(completed.targets, completed.translations);
    if !ops.is_empty() {
        session.apply(Op::Batch {
            ops,
            label: format!("{}: batch {}", info.id, completed.index + 1),
        })?;
    }
    Ok(())
}

fn translation_ops(targets: Vec<TranslationTarget>, translations: Vec<String>) -> Vec<Op> {
    targets
        .into_iter()
        .zip(translations)
        .map(|(target, translation)| {
            let translation = restore_text(&translation, &target.replacements);
            let translation = normalize_translation_line_breaks(&translation);
            Op::UpdateNode {
                page: target.page_id,
                id: target.node_id,
                patch: NodePatch {
                    data: Some(NodeDataPatch::Text(TextDataPatch {
                        translation: Some(Some(translation)),
                        ..Default::default()
                    })),
                    transform: None,
                    visible: None,
                },
                prev: NodePatch::default(),
            }
        })
        .collect()
}

fn normalize_translation_line_breaks(text: &str) -> String {
    text.replace("<br />", "\n")
        .replace("<br/>", "\n")
        .replace("<br>", "\n")
        .replace("\\n", "\n")
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
    warning_count: &mut usize,
    sink: Option<&WarningSink>,
) {
    let _ = total_steps;
    tracing::warn!(
        engine = engine_id,
        page = %page_id,
        step_index,
        "pipeline step failed: {err:#}"
    );
    *warning_count += 1;
    if let Some(sink) = sink {
        sink(WarningTick {
            step_id: engine_id.to_string(),
            page_index,
            total_pages,
            message: format!("{err:#}"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(source: &str) -> TranslationTarget {
        TranslationTarget {
            page_id: PageId::new(),
            node_id: NodeId::new(),
            source: source.to_string(),
            replacements: Vec::new(),
        }
    }

    fn target_with_replacement(
        source: &str,
        placeholder: &str,
        replacement: &str,
    ) -> TranslationTarget {
        TranslationTarget {
            page_id: PageId::new(),
            node_id: NodeId::new(),
            source: source.to_string(),
            replacements: vec![PlaceholderReplacement {
                placeholder: placeholder.to_string(),
                target: replacement.to_string(),
            }],
        }
    }

    fn drain_batches(
        mut targets: Vec<TranslationTarget>,
        char_limit: usize,
    ) -> Vec<Vec<TranslationTarget>> {
        let mut batches = Vec::new();
        while !targets.is_empty() {
            batches.push(take_next_translation_batch(&mut targets, char_limit));
        }
        batches
    }

    #[test]
    fn translation_batches_respect_limit_without_splitting_blocks() {
        let targets = vec![target("abcd"), target("efg"), target("hijkl")];

        let batches = drain_batches(targets, 8);

        assert_eq!(batches.len(), 2);
        assert_eq!(
            batches[0]
                .iter()
                .map(|t| t.source.as_str())
                .collect::<Vec<_>>(),
            vec!["abcd", "efg"]
        );
        assert_eq!(
            batches[1]
                .iter()
                .map(|t| t.source.as_str())
                .collect::<Vec<_>>(),
            vec!["hijkl"]
        );
    }

    #[test]
    fn oversized_translation_block_stays_whole() {
        let targets = vec![target("abcdef")];

        let batches = drain_batches(targets, 3);

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0][0].source, "abcdef");
    }

    #[test]
    fn batch_translation_state_waits_for_texts_before_render() {
        let page = PageId::new();
        let mut states = BatchPageStates::new(&[page]);
        states.mark_pre_done(page, 2);

        assert!(!states.ready_for_render(page));
        states.mark_inpaint_done(page);
        assert!(!states.ready_for_render(page));
        states.mark_translated(page, 1);
        assert!(!states.ready_for_render(page));
        states.mark_translated(page, 1);
        assert!(states.ready_for_render(page));
    }

    #[test]
    fn batch_translation_ops_restore_terminology_placeholders() {
        let target = target_with_replacement("{{1}} arrives", "{{1}}", "艾莉絲");

        let ops = translation_ops(vec![target], vec!["{{1}} has arrived".to_string()]);

        let [Op::UpdateNode { patch, .. }] = ops.as_slice() else {
            panic!("expected one update op");
        };
        let Some(NodeDataPatch::Text(text)) = patch.data.as_ref() else {
            panic!("expected text patch");
        };
        assert_eq!(
            text.translation.as_ref().and_then(|value| value.as_ref()),
            Some(&"艾莉絲 has arrived".to_string())
        );
    }

    #[test]
    fn batch_translation_prompt_includes_placeholder_rule_when_terminology_is_active() {
        let options = PipelineRunOptions {
            target_language: Some("English".to_string()),
            terminology: vec![crate::terminology::ActiveGlossary {
                priority: 10,
                terms: vec![crate::terminology::TerminologyEntry {
                    source: "Alice".to_string(),
                    target: "艾莉絲".to_string(),
                }],
            }],
            ..Default::default()
        };

        let prompt = batch_translation_system_prompt(&options).expect("prompt should be present");

        assert!(prompt.contains("Strictly preserve all placeholders like {{1}}, {{2}}"));
    }

    #[test]
    fn batch_translation_ops_normalize_line_break_markers() {
        let target = target("source");

        let ops = translation_ops(vec![target], vec![r"first<br>second\nthird".to_string()]);

        let [Op::UpdateNode { patch, .. }] = ops.as_slice() else {
            panic!("expected one update op");
        };
        let Some(NodeDataPatch::Text(text)) = patch.data.as_ref() else {
            panic!("expected text patch");
        };
        assert_eq!(
            text.translation.as_ref().and_then(|value| value.as_ref()),
            Some(&"first\nsecond\nthird".to_string())
        );
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
