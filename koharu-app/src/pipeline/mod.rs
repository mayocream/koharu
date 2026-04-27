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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};
use koharu_core::{NodeDataPatch, NodeId, NodePatch, Op, PageId, PipelineStep, TextDataPatch};
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

    let total_pages = pages.len().max(1);
    let mut total_units = (total_pages * (pre_steps.len() + post_steps.len()) + 1).max(1);
    let mut completed = 0usize;
    let mut warning_count = 0usize;
    let mut skipped_pages = Vec::new();
    let mut last_percent = 0u8;

    for (page_index, page_id) in pages.iter().enumerate() {
        for (seq, &i) in pre_steps.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            let info = infos[i];
            emit_progress(
                progress.as_ref(),
                info,
                seq,
                pre_steps.len() + post_steps.len() + 1,
                page_index,
                total_pages,
                completed,
                total_units,
                &mut last_percent,
            );

            if !session.scene.read().pages.contains_key(page_id) {
                skipped_pages.push(*page_id);
                completed += pre_steps.len() - seq;
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
                pre_steps.len() + post_steps.len() + 1,
                &mut warning_count,
                warnings.as_ref(),
            )
            .await?;
            completed += 1;
            if !ok {
                skipped_pages.push(*page_id);
                break;
            }
        }
    }

    let mut targets = Vec::new();
    let scene_snap = session.scene_snapshot();
    for page_id in &pages {
        if skipped_pages.contains(page_id) {
            continue;
        }
        targets.extend(collect_translation_targets(&scene_snap, *page_id));
    }
    let batches = build_translation_batches(targets, batch_char_limit);
    total_units = (total_pages * (pre_steps.len() + post_steps.len()) + batches.len()).max(1);

    for (batch_index, batch) in batches.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let info = infos[translation_steps[0]];
        emit_progress(
            progress.as_ref(),
            info,
            pre_steps.len(),
            pre_steps.len() + post_steps.len() + 1,
            batch_index,
            total_pages,
            completed,
            total_units,
            &mut last_percent,
        );
        let sources = batch
            .iter()
            .map(|target| target.source.clone())
            .collect::<Vec<_>>();
        let translations = llm
            .translate_xml_tagged_texts(
                &sources,
                spec.options.target_language.as_deref(),
                spec.options.system_prompt.as_deref(),
            )
            .await?;
        let ops = translation_ops(batch, translations);
        if !ops.is_empty() {
            session.apply(Op::Batch {
                ops,
                label: format!("{}: batch {}", info.id, batch_index + 1),
            })?;
        }
        completed += 1;
    }

    for (page_index, page_id) in pages.iter().enumerate() {
        if skipped_pages.contains(page_id) {
            continue;
        }
        for (seq, &i) in post_steps.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled");
            }
            let info = infos[i];
            emit_progress(
                progress.as_ref(),
                info,
                pre_steps.len() + 1 + seq,
                pre_steps.len() + post_steps.len() + 1,
                page_index,
                total_pages,
                completed,
                total_units,
                &mut last_percent,
            );
            if !session.scene.read().pages.contains_key(page_id) {
                completed += post_steps.len() - seq;
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
                pre_steps.len() + post_steps.len() + 1,
                &mut warning_count,
                warnings.as_ref(),
            )
            .await?;
            completed += 1;
            if !ok {
                break;
            }
        }
    }

    if let Some(sink) = progress.as_ref() {
        sink(ProgressTick {
            step: None,
            step_id: String::new(),
            step_index: pre_steps.len() + post_steps.len(),
            total_steps: pre_steps.len() + post_steps.len() + 1,
            page_index: total_pages.saturating_sub(1),
            total_pages,
            overall_percent: 100,
        });
    }

    Ok(RunOutcome { warning_count })
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
}

fn collect_translation_targets(
    scene: &koharu_core::Scene,
    page_id: PageId,
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
            Some(TranslationTarget {
                page_id,
                node_id: *node_id,
                source: source.clone(),
            })
        })
        .collect()
}

fn build_translation_batches(
    targets: Vec<TranslationTarget>,
    char_limit: usize,
) -> Vec<Vec<TranslationTarget>> {
    let char_limit = char_limit.max(1);
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_chars = 0usize;

    for target in targets {
        let target_chars = target.source.chars().count();
        if !current.is_empty() && current_chars + target_chars > char_limit {
            batches.push(current);
            current = Vec::new();
            current_chars = 0;
        }
        current_chars += target_chars;
        current.push(target);
    }

    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

fn translation_ops(targets: Vec<TranslationTarget>, translations: Vec<String>) -> Vec<Op> {
    targets
        .into_iter()
        .zip(translations)
        .map(|(target, translation)| Op::UpdateNode {
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
        })
        .collect()
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
        }
    }

    #[test]
    fn translation_batches_respect_limit_without_splitting_blocks() {
        let targets = vec![target("abcd"), target("efg"), target("hijkl")];

        let batches = build_translation_batches(targets, 8);

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

        let batches = build_translation_batches(targets, 3);

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0][0].source, "abcdef");
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
