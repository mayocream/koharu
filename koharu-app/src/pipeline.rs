use std::str::FromStr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use koharu_core::commands::ProcessRequest;
use koharu_core::events::{PipelineProgress, PipelineStatus, PipelineStep};
use koharu_llm::ModelId;
use once_cell::sync::Lazy;
use tokio::sync::broadcast;

use crate::{
    AppResources,
    state_tx::{self, ChangedField, ProjectStage},
};

pub struct PipelineHandle {
    pub id: String,
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
    job_id: String,
) {
    let result = run_pipeline_inner(&resources, &request, &cancel, &job_id).await;

    let total_docs = match &request.indices {
        Some(indices) => indices.len(),
        None => match request.index {
            Some(_) => 1,
            None => state_tx::doc_count(&resources.state).await,
        },
    };

    match result {
        Ok(()) if cancel.load(Ordering::Relaxed) => {
            emit(PipelineProgress {
                job_id: job_id.clone(),
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
                job_id: job_id.clone(),
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
                job_id: job_id.clone(),
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
    job_id: &str,
) -> anyhow::Result<()> {
    let docs_to_process = {
        let len = state_tx::doc_count(&res.state).await;
        match &req.indices {
            Some(indices) => {
                for &i in indices {
                    if i >= len {
                        anyhow::bail!("Document index {i} out of range (have {len})");
                    }
                }
                indices.clone()
            }
            None => match req.index {
                Some(i) if i >= len => {
                    anyhow::bail!("Document index {i} out of range (have {len})")
                }
                Some(i) => vec![i],
                None => (0..len).collect(),
            },
        }
    };
    let total_docs = docs_to_process.len();

    if total_docs == 0 {
        return Ok(());
    }

    if let Some(model_id) = &req.llm_model_id
        && !res.llm.ready().await
    {
        if model_id.contains(':') {
            let (provider_id, model_part) = model_id.split_once(':').unwrap();
            res.llm
                .load_api(
                    provider_id,
                    model_part,
                    koharu_llm::providers::ProviderConfig {
                        http_client: res.runtime.http_client(),
                        api_key: req.llm_api_key.clone(),
                        base_url: req.llm_base_url.clone(),
                        temperature: req.llm_temperature,
                        max_tokens: req.llm_max_tokens,
                        custom_system_prompt: req.llm_custom_system_prompt.clone(),
                    },
                )
                .await?;
        } else {
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
    }

    let total_steps = PipelineStep::ALL.len();

    for (doc_ordinal, &doc_index) in docs_to_process.iter().enumerate() {
        for (step_ordinal, step) in PipelineStep::ALL.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }

            let overall = compute_percent(doc_ordinal, step_ordinal, total_docs, total_steps);
            emit(PipelineProgress {
                job_id: job_id.to_string(),
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

            let step_result = async {
                match step {
                    PipelineStep::Detect => res.ml.detect(&mut snapshot).await,
                    PipelineStep::Ocr => res.ml.ocr(&mut snapshot).await,
                    PipelineStep::Inpaint => res.ml.inpaint(&mut snapshot).await,
                    PipelineStep::LlmGenerate => {
                        res.llm
                            .translate(&mut snapshot, req.language.as_deref())
                            .await
                    }
                    PipelineStep::Render => {
                        res.renderer.render(
                            &mut snapshot,
                            None,
                            req.shader_effect.unwrap_or_default(),
                            req.shader_stroke.clone(),
                            req.font_family.as_deref(),
                        )?;
                        Ok(())
                    }
                }
            }
            .await;

            if let Err(err) = step_result {
                let _ = state_tx::mark_stage_failure(
                    &res.state,
                    doc_index,
                    map_pipeline_stage(*step),
                    err.to_string(),
                )
                .await;
                return Err(err);
            }

            let changed = match step {
                PipelineStep::Detect => &[ChangedField::TextBlocks, ChangedField::Segment][..],
                PipelineStep::Ocr => &[ChangedField::TextBlocks][..],
                PipelineStep::Inpaint => &[ChangedField::Inpainted][..],
                PipelineStep::LlmGenerate => &[ChangedField::TextBlocks][..],
                PipelineStep::Render => &[ChangedField::TextBlocks, ChangedField::Rendered][..],
            };
            state_tx::update_doc(&res.state, doc_index, snapshot, changed).await?;
            state_tx::mark_stage_success(&res.state, doc_index, map_pipeline_stage(*step)).await?;
        }
    }

    Ok(())
}

fn map_pipeline_stage(step: PipelineStep) -> ProjectStage {
    match step {
        PipelineStep::Detect => ProjectStage::Detect,
        PipelineStep::Ocr => ProjectStage::Ocr,
        PipelineStep::Inpaint => ProjectStage::Inpaint,
        PipelineStep::LlmGenerate => ProjectStage::Translate,
        PipelineStep::Render => ProjectStage::Render,
    }
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
