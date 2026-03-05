use std::str::FromStr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use koharu_api::commands::ProcessRequest;
use koharu_api::events::{PipelineProgress, PipelineStatus, PipelineStep};
use koharu_ml::llm::ModelId;
use once_cell::sync::Lazy;
use tokio::sync::broadcast;

use crate::{AppResources, state_tx};

pub struct PipelineHandle {
    pub cancel: Arc<AtomicBool>,
}

static PIPELINE_TX: Lazy<broadcast::Sender<PipelineProgress>> =
    Lazy::new(|| broadcast::channel(256).0);

pub fn subscribe() -> broadcast::Receiver<PipelineProgress> {
    PIPELINE_TX.subscribe()
}

fn emit(progress: PipelineProgress) {
    let _ = PIPELINE_TX.send(progress);
}

fn compute_percent(doc: usize, step: usize, total_docs: usize, total_steps: usize) -> u8 {
    let done_units = doc * total_steps + step;
    let total_units = total_docs * total_steps;
    if total_units == 0 {
        return 0;
    }
    ((done_units as f64 / total_units as f64) * 100.0).round() as u8
}

pub async fn run_pipeline(
    resources: AppResources,
    request: ProcessRequest,
    cancel: Arc<AtomicBool>,
) {
    let result = run_pipeline_inner(&resources, &request, &cancel).await;

    let total_docs = match request.index {
        Some(_) => 1,
        None => resources.state.read().await.documents.len(),
    };

    match result {
        Ok(()) if cancel.load(Ordering::Relaxed) => {
            emit(PipelineProgress {
                status: PipelineStatus::Cancelled,
                step: None,
                current_document: total_docs,
                total_documents: total_docs,
                current_step_index: 0,
                total_steps: PipelineStep::ALL.len(),
                overall_percent: 0,
            });
        }
        Ok(()) => {
            emit(PipelineProgress {
                status: PipelineStatus::Completed,
                step: None,
                current_document: total_docs,
                total_documents: total_docs,
                current_step_index: PipelineStep::ALL.len(),
                total_steps: PipelineStep::ALL.len(),
                overall_percent: 100,
            });
        }
        Err(err) => {
            tracing::error!("Pipeline failed: {err:#}");
            emit(PipelineProgress {
                status: PipelineStatus::Failed(err.to_string()),
                step: None,
                current_document: 0,
                total_documents: total_docs,
                current_step_index: 0,
                total_steps: PipelineStep::ALL.len(),
                overall_percent: 0,
            });
        }
    }

    let mut guard = resources.pipeline.write().await;
    *guard = None;
}

async fn run_pipeline_inner(
    res: &AppResources,
    req: &ProcessRequest,
    cancel: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let total_docs = {
        let guard = res.state.read().await;
        let len = guard.documents.len();
        match req.index {
            Some(i) if i >= len => anyhow::bail!("Document index {i} out of range (have {len})"),
            Some(_) => 1,
            None => len,
        }
    };

    if total_docs == 0 {
        return Ok(());
    }

    if let Some(model_id) = &req.llm_model_id
        && !res.llm.ready().await
    {
        let id = ModelId::from_str(model_id)?;
        res.llm.load(id).await;
        for _ in 0..300 {
            if res.llm.ready().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
        }
        if !res.llm.ready().await {
            anyhow::bail!("LLM failed to load within timeout");
        }
    }

    let start_index = req.index.unwrap_or(0);
    let end_index = req.index.map(|i| i + 1).unwrap_or(total_docs);
    let total_steps = PipelineStep::ALL.len();

    for (doc_ordinal, doc_index) in (start_index..end_index).enumerate() {
        for (step_ordinal, step) in PipelineStep::ALL.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }

            let overall = compute_percent(doc_ordinal, step_ordinal, total_docs, total_steps);
            emit(PipelineProgress {
                status: PipelineStatus::Running,
                step: Some(*step),
                current_document: doc_ordinal,
                total_documents: total_docs,
                current_step_index: step_ordinal,
                total_steps,
                overall_percent: overall,
            });

            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;

            let mut snapshot = state_tx::read_doc(&res.state, doc_index).await?;

            match step {
                PipelineStep::Detect => res.ml.detect(&mut snapshot).await?,
                PipelineStep::Ocr => res.ml.ocr(&mut snapshot).await?,
                PipelineStep::Inpaint => res.ml.inpaint(&mut snapshot).await?,
                PipelineStep::LlmGenerate => {
                    res.llm
                        .translate(&mut snapshot, req.language.as_deref())
                        .await?;
                }
                PipelineStep::Render => {
                    res.renderer.render(
                        &mut snapshot,
                        None,
                        req.shader_effect.unwrap_or_default(),
                        req.font_family.as_deref(),
                    )?;
                }
            }

            state_tx::update_doc(&res.state, doc_index, snapshot).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compute_percent;

    #[test]
    fn compute_percent_handles_zero_units() {
        assert_eq!(compute_percent(0, 0, 0, 5), 0);
        assert_eq!(compute_percent(0, 0, 2, 0), 0);
    }

    #[test]
    fn compute_percent_progresses_monotonically() {
        let total_docs = 2;
        let total_steps = 5;
        let first = compute_percent(0, 0, total_docs, total_steps);
        let middle = compute_percent(0, 3, total_docs, total_steps);
        let last = compute_percent(1, 4, total_docs, total_steps);
        assert!(first < middle);
        assert!(middle < last);
        assert_eq!(last, 90);
    }
}
