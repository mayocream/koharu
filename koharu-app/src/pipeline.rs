use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use koharu_core::commands::ProcessRequest;
use koharu_core::events::{PipelineStatus, PipelineStep};
use koharu_core::{JobState, JobStatus};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AppResources;
use crate::llm::ensure_target_ready;

pub type Jobs = Arc<RwLock<HashMap<String, JobState>>>;

pub struct PipelineHandle {
    pub id: String,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
struct JobProgress {
    current_document: usize,
    total_documents: usize,
    current_step_index: usize,
    total_steps: usize,
    overall_percent: u8,
}

fn compute_percent(doc: usize, step: usize, total_docs: usize, total_steps: usize) -> u8 {
    let done_units = doc * total_steps + step;
    let total_units = total_docs * total_steps;
    if total_units == 0 {
        return 0;
    }
    ((done_units as f64 / total_units as f64) * 100.0).round() as u8
}

async fn update_job(jobs: &Jobs, job: JobState) {
    let terminal = !matches!(job.status, JobStatus::Running);
    let mut guard = jobs.write().await;
    if terminal {
        guard.remove(&job.id);
    } else {
        guard.insert(job.id.clone(), job);
    }
}

fn make_job_state(
    job_id: &str,
    status: PipelineStatus,
    step: Option<PipelineStep>,
    progress: JobProgress,
) -> JobState {
    let (job_status, error) = match status {
        PipelineStatus::Running => (JobStatus::Running, None),
        PipelineStatus::Completed => (JobStatus::Completed, None),
        PipelineStatus::Cancelled => (JobStatus::Cancelled, None),
        PipelineStatus::Failed(message) => (JobStatus::Failed, Some(message)),
    };

    JobState {
        id: job_id.to_string(),
        kind: "pipeline".to_string(),
        status: job_status,
        step: step.map(|step| step.to_string()),
        current_document: progress.current_document,
        total_documents: progress.total_documents,
        current_step_index: progress.current_step_index,
        total_steps: progress.total_steps,
        overall_percent: progress.overall_percent,
        error,
    }
}

pub async fn process(
    state: AppResources,
    payload: ProcessRequest,
    jobs: Jobs,
) -> anyhow::Result<String> {
    {
        let guard = state.pipeline.read().await;
        if guard.is_some() {
            anyhow::bail!("A processing pipeline is already running");
        }
    }

    let job_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.pipeline.write().await;
        *guard = Some(PipelineHandle {
            id: job_id.clone(),
            cancel: cancel.clone(),
        });
    }

    let resources = state.clone();
    let job_id_for_task = job_id.clone();
    tokio::spawn(async move {
        run_pipeline(resources, payload, cancel, job_id_for_task, jobs).await;
    });

    Ok(job_id)
}

pub async fn process_cancel(state: AppResources) -> anyhow::Result<()> {
    let guard = state.pipeline.read().await;
    if let Some(handle) = guard.as_ref() {
        handle.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

async fn run_pipeline(
    resources: AppResources,
    request: ProcessRequest,
    cancel: Arc<AtomicBool>,
    job_id: String,
    jobs: Jobs,
) {
    let result = run_pipeline_inner(&resources, &request, &cancel, &job_id, &jobs).await;

    let total_docs = match request.document_id {
        Some(_) => 1,
        None => resources
            .cache
            .list_documents()
            .map(|entries| entries.len())
            .unwrap_or(0),
    };

    let final_state = match result {
        Ok(()) if cancel.load(Ordering::Relaxed) => make_job_state(
            &job_id,
            PipelineStatus::Cancelled,
            None,
            JobProgress {
                current_document: total_docs,
                total_documents: total_docs,
                current_step_index: 0,
                total_steps: PipelineStep::ALL.len(),
                overall_percent: 0,
            },
        ),
        Ok(()) => make_job_state(
            &job_id,
            PipelineStatus::Completed,
            None,
            JobProgress {
                current_document: total_docs,
                total_documents: total_docs,
                current_step_index: PipelineStep::ALL.len(),
                total_steps: PipelineStep::ALL.len(),
                overall_percent: 100,
            },
        ),
        Err(err) => {
            tracing::error!("Pipeline failed: {err:#}");
            make_job_state(
                &job_id,
                PipelineStatus::Failed(err.to_string()),
                None,
                JobProgress {
                    current_document: 0,
                    total_documents: total_docs,
                    current_step_index: 0,
                    total_steps: PipelineStep::ALL.len(),
                    overall_percent: 0,
                },
            )
        }
    };
    update_job(&jobs, final_state).await;

    let mut guard = resources.pipeline.write().await;
    *guard = None;
}

async fn run_pipeline_inner(
    res: &AppResources,
    req: &ProcessRequest,
    cancel: &Arc<AtomicBool>,
    job_id: &str,
    jobs: &Jobs,
) -> anyhow::Result<()> {
    let entries = res.cache.list_documents()?;
    let page_ids: Vec<String> = match req.document_id.as_deref() {
        Some(id) => {
            if !entries.iter().any(|p| p.id == id) {
                anyhow::bail!("Document not found: {id}");
            }
            vec![id.to_string()]
        }
        None => entries.into_iter().map(|p| p.id).collect(),
    };

    let total_docs = page_ids.len();
    if total_docs == 0 {
        return Ok(());
    }

    if let Some(llm) = &req.llm {
        ensure_target_ready(res, &llm.target, llm.options.as_ref()).await?;
    }

    let total_steps = PipelineStep::ALL.len();

    for (doc_ordinal, page_id) in page_ids.iter().enumerate() {
        let mut doc = res.cache.get(page_id).await?;

        for (step_ordinal, step) in PipelineStep::ALL.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }

            let overall = compute_percent(doc_ordinal, step_ordinal, total_docs, total_steps);
            update_job(
                jobs,
                make_job_state(
                    job_id,
                    PipelineStatus::Running,
                    Some(*step),
                    JobProgress {
                        current_document: doc_ordinal,
                        total_documents: total_docs,
                        current_step_index: step_ordinal,
                        total_steps,
                        overall_percent: overall,
                    },
                ),
            )
            .await;

            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;

            match step {
                PipelineStep::Detect => res.ml.detect(&mut doc).await?,
                PipelineStep::Ocr => res.ml.ocr(&mut doc).await?,
                PipelineStep::Inpaint => res.ml.inpaint(&mut doc).await?,
                PipelineStep::LlmGenerate => {
                    res.llm.translate(&mut doc, req.language.as_deref()).await?;
                }
                PipelineStep::Render => {
                    res.renderer.render(
                        &mut doc,
                        None,
                        req.shader_effect.unwrap_or_default(),
                        req.shader_stroke.clone(),
                        req.font_family.as_deref(),
                    )?;
                }
            }

            // Save after each step (crash-safe)
            res.cache.put(&doc).await?;
        }
        // Evict from cache to free memory
        res.cache.evict(page_id).await;
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
