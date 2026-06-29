//! Pipeline: runs an ordered set of engines across one or more pages and
//! wraps each engine's output in one `Op::Batch` before applying via the
//! session's history.
//!
//! **Engines don't mutate the scene.** They return `Vec<Op>`; this driver
//! applies them transactionally (per-engine) against the active session.

pub mod artifacts;
pub mod chapter_translate;
pub mod engine;
mod engines;

pub use artifacts::Artifact;
pub use engine::{
    BoxFuture, Engine, EngineCtx, EngineInfo, EngineLoadFn, PipelineRunOptions, Registry,
    build_order,
};
pub use engines::support;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};
use koharu_core::{Op, PageId, PipelineStep};
use koharu_runtime::RuntimeManager;
use tracing::Instrument;

use crate::pipeline::chapter_translate::{
    blocks_to_ops, collect_blocks, log_chapter_translation_plan, translate_blocks_chunked,
    ChapterChunkConfig,
};

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
}

#[derive(Debug, Clone)]
pub enum Scope {
    WholeProject,
    Pages(Vec<PageId>),
}

// ---------------------------------------------------------------------------
// Chapter-mode step partitioning
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct StepPlan {
    detect: Vec<usize>,
    ocr: Vec<usize>,
    has_translate: bool,
    inpaint: Vec<usize>,
    render: Vec<usize>,
}

fn partition_steps(order: &[usize], infos: &[&EngineInfo]) -> StepPlan {
    let mut plan = StepPlan::default();
    for &idx in order {
        let info = infos[idx];
        if info.produces.contains(&Artifact::Translations) {
            plan.has_translate = true;
            continue;
        }
        match step_for(info) {
            Some(PipelineStep::Detect) => plan.detect.push(idx),
            Some(PipelineStep::Ocr) => plan.ocr.push(idx),
            Some(PipelineStep::Inpaint) => plan.inpaint.push(idx),
            Some(PipelineStep::Render) => plan.render.push(idx),
            Some(PipelineStep::LlmGenerate) => plan.has_translate = true,
            None => {}
        }
    }
    plan
}

fn use_chapter_mode(options: &PipelineRunOptions) -> bool {
    options.chapter_context_translation && options.text_node_ids.is_none()
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

    if use_chapter_mode(&spec.options) {
        return run_chapter_mode(
            session,
            registry,
            runtime,
            cpu,
            llm,
            renderer,
            spec,
            cancel,
            progress,
            warnings,
            infos,
            order,
            pages,
            total_pages,
            total_steps,
        )
        .await;
    }

    run_sequential(
        session,
        registry,
        runtime,
        cpu,
        llm,
        renderer,
        spec,
        cancel,
        progress,
        warnings,
        infos,
        order,
        pages,
        total_pages,
        total_steps,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_sequential(
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
    infos: Vec<&EngineInfo>,
    order: Vec<usize>,
    pages: Vec<PageId>,
    total_pages: usize,
    total_steps: usize,
) -> Result<RunOutcome> {
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

    emit_final_progress(progress.as_ref(), total_steps, total_pages);
    Ok(RunOutcome { warning_count })
}

#[allow(clippy::too_many_arguments)]
async fn run_chapter_mode(
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
    infos: Vec<&EngineInfo>,
    order: Vec<usize>,
    pages: Vec<PageId>,
    total_pages: usize,
    total_steps: usize,
) -> Result<RunOutcome> {
    let plan = partition_steps(&order, &infos);
    let total_units = (total_pages * total_steps) as u64;
    let mut completed: u64 = 0;
    let mut warning_count: usize = 0;
    let mut failed_pages = HashSet::new();

    for &engine_idx in &plan.detect {
        run_step_for_all_pages(
            &session,
            &registry,
            &runtime,
            cpu,
            &llm,
            &renderer,
            &spec,
            &cancel,
            progress.as_ref(),
            warnings.as_ref(),
            &infos,
            &order,
            &pages,
            &mut failed_pages,
            engine_idx,
            total_pages,
            total_steps,
            total_units,
            &mut completed,
            &mut warning_count,
        )
        .await?;
    }

    for &engine_idx in &plan.ocr {
        run_step_for_all_pages(
            &session,
            &registry,
            &runtime,
            cpu,
            &llm,
            &renderer,
            &spec,
            &cancel,
            progress.as_ref(),
            warnings.as_ref(),
            &infos,
            &order,
            &pages,
            &mut failed_pages,
            engine_idx,
            total_pages,
            total_steps,
            total_units,
            &mut completed,
            &mut warning_count,
        )
        .await?;
    }

    if plan.has_translate {
        run_chapter_translation(
            &session,
            &llm,
            &spec,
            &cancel,
            progress.as_ref(),
            &pages,
            &failed_pages,
            total_pages,
            total_steps,
            total_units,
            &mut completed,
        )
        .await?;
    }

    for &engine_idx in &plan.inpaint {
        run_step_for_all_pages(
            &session,
            &registry,
            &runtime,
            cpu,
            &llm,
            &renderer,
            &spec,
            &cancel,
            progress.as_ref(),
            warnings.as_ref(),
            &infos,
            &order,
            &pages,
            &mut failed_pages,
            engine_idx,
            total_pages,
            total_steps,
            total_units,
            &mut completed,
            &mut warning_count,
        )
        .await?;
    }

    for &engine_idx in &plan.render {
        run_step_for_all_pages(
            &session,
            &registry,
            &runtime,
            cpu,
            &llm,
            &renderer,
            &spec,
            &cancel,
            progress.as_ref(),
            warnings.as_ref(),
            &infos,
            &order,
            &pages,
            &mut failed_pages,
            engine_idx,
            total_pages,
            total_steps,
            total_units,
            &mut completed,
            &mut warning_count,
        )
        .await?;
    }

    emit_final_progress(progress.as_ref(), total_steps, total_pages);
    Ok(RunOutcome { warning_count })
}

#[allow(clippy::too_many_arguments)]
async fn run_step_for_all_pages(
    session: &Arc<ProjectSession>,
    registry: &Arc<Registry>,
    runtime: &Arc<RuntimeManager>,
    cpu: bool,
    llm: &Arc<llm::Model>,
    renderer: &Arc<renderer::Renderer>,
    spec: &PipelineSpec,
    cancel: &Arc<AtomicBool>,
    progress: Option<&ProgressSink>,
    warnings: Option<&WarningSink>,
    infos: &[&EngineInfo],
    order: &[usize],
    pages: &[PageId],
    failed_pages: &mut HashSet<PageId>,
    engine_idx: usize,
    total_pages: usize,
    total_steps: usize,
    total_units: u64,
    completed: &mut u64,
    warning_count: &mut usize,
) -> Result<()> {
    let info = infos[engine_idx];
    let seq = order
        .iter()
        .position(|&idx| idx == engine_idx)
        .unwrap_or(0);

    for (page_index, page_id) in pages.iter().enumerate() {
        if failed_pages.contains(page_id) {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }

        if let Some(sink) = progress {
            let percent = ((*completed * 100) / total_units).min(100) as u8;
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

        if !session.scene.read().pages.contains_key(page_id) {
            failed_pages.insert(*page_id);
            continue;
        }

        let engine = match registry.get(info.id, runtime, cpu).await {
            Ok(e) => e,
            Err(err) => {
                report_step_failure(
                    info.id,
                    page_id,
                    seq,
                    page_index,
                    total_pages,
                    total_steps,
                    &err,
                    warning_count,
                    warnings,
                );
                failed_pages.insert(*page_id);
                continue;
            }
        };
        let scene_snap = session.scene_snapshot();
        let ctx = EngineCtx {
            scene: &scene_snap,
            page: *page_id,
            blobs: &session.blobs,
            runtime,
            cancel,
            options: &spec.options,
            llm,
            renderer,
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
                    warning_count,
                    warnings,
                );
                failed_pages.insert(*page_id);
                continue;
            }
        };
        *completed += 1;
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
                warning_count,
                warnings,
            );
            failed_pages.insert(*page_id);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_chapter_translation(
    session: &Arc<ProjectSession>,
    llm: &Arc<llm::Model>,
    spec: &PipelineSpec,
    cancel: &Arc<AtomicBool>,
    progress: Option<&ProgressSink>,
    pages: &[PageId],
    failed_pages: &HashSet<PageId>,
    total_pages: usize,
    total_steps: usize,
    total_units: u64,
    completed: &mut u64,
) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("cancelled");
    }

    if let Some(sink) = progress {
        let percent = ((*completed * 100) / total_units).min(100) as u8;
        sink(ProgressTick {
            step: Some(PipelineStep::LlmGenerate),
            step_id: "chapter-translate".to_string(),
            step_index: 0,
            total_steps,
            page_index: 0,
            total_pages,
            overall_percent: percent,
        });
    }

    let scene = session.scene_snapshot();
    let blocks = collect_blocks(&scene, pages, failed_pages);
    let chunk_config = ChapterChunkConfig::resolve(
        spec.options.chapter_translation_token_budget,
        spec.options.chapter_translation_max_blocks,
    );
    log_chapter_translation_plan(pages.len(), &blocks, chunk_config);
    if blocks.is_empty() {
        *completed += total_pages as u64;
        return Ok(());
    }

    let translations = translate_blocks_chunked(
        llm,
        &blocks,
        chunk_config,
        spec.options.target_language.as_deref(),
        spec.options.system_prompt.as_deref(),
    )
    .await?;

    let ops = blocks_to_ops(&blocks, &translations);
    if !ops.is_empty() {
        session.apply(Op::Batch {
            ops,
            label: "chapter-translate".to_string(),
        })?;
    }

    *completed += total_pages as u64;
    Ok(())
}

fn emit_final_progress(
    progress: Option<&ProgressSink>,
    total_steps: usize,
    total_pages: usize,
) {
    if let Some(sink) = progress {
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
    use koharu_core::NodeId;

    fn chapter_mode_test_steps() -> Vec<String> {
        vec![
            "pp-doclayout-v3".to_string(),
            "comic-text-detector-seg".to_string(),
            "speech-bubble-segmentation".to_string(),
            "yuzumarker-font-detection".to_string(),
            "paddle-ocr-vl-1.6".to_string(),
            "llm".to_string(),
            "lama-manga".to_string(),
            "koharu-renderer".to_string(),
        ]
    }

    #[test]
    fn catalog_includes_anime_text_detector() {
        let catalog = catalog();

        assert!(catalog.detectors.iter().any(|engine| {
            engine.id == "anime-text"
                && engine.name == "Anime Text YOLO (N)"
                && engine.produces.iter().map(String::as_str).eq(["TextBoxes"])
        }));
    }

    #[test]
    fn use_chapter_mode_requires_flag_without_text_node_scope() {
        assert!(!use_chapter_mode(&PipelineRunOptions {
            chapter_context_translation: false,
            ..Default::default()
        }));
        assert!(use_chapter_mode(&PipelineRunOptions {
            chapter_context_translation: true,
            ..Default::default()
        }));
        assert!(!use_chapter_mode(&PipelineRunOptions {
            chapter_context_translation: true,
            text_node_ids: Some(vec![NodeId::default()]),
            ..Default::default()
        }));
    }

    #[test]
    fn partition_steps_separates_translator_from_other_phases() {
        let steps = chapter_mode_test_steps();
        let infos: Vec<&EngineInfo> = steps
            .iter()
            .map(|id| Registry::find(id))
            .collect::<Result<_>>()
            .unwrap();
        let order = build_order(&infos).unwrap();
        let plan = partition_steps(&order, &infos);

        assert_eq!(plan.detect.len(), 4);
        assert_eq!(plan.ocr.len(), 1);
        assert!(plan.has_translate);
        assert_eq!(plan.inpaint.len(), 1);
        assert_eq!(plan.render.len(), 1);
        assert!(plan
            .detect
            .iter()
            .all(|idx| !infos[*idx].produces.contains(&Artifact::Translations)));
    }
}
