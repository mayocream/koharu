use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use koharu_core::{JobState, JobStatus};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AppResources;
use crate::engine;

pub type Jobs = Arc<RwLock<HashMap<String, JobState>>>;

pub struct PipelineHandle {
    pub id: String,
    pub cancel: Arc<AtomicBool>,
}

pub async fn process(
    state: AppResources,
    payload: koharu_core::commands::ProcessRequest,
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

    let jid = job_id.clone();
    tokio::spawn(async move {
        run(state, payload, cancel, jid, jobs).await;
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

#[tracing::instrument(level = "info", skip_all, fields(%job_id))]
async fn run(
    res: AppResources,
    request: koharu_core::commands::ProcessRequest,
    cancel: Arc<AtomicBool>,
    job_id: String,
    jobs: Jobs,
) {
    let result = run_inner(&res, &request, &cancel, &job_id, &jobs).await;

    if let Err(err) = &result {
        tracing::error!(error = %err, "pipeline failed");
    }

    let total_docs = match request.document_id {
        Some(_) => 1,
        None => res.storage.page_count().await,
    };
    let config = res.config.read().await.clone();
    let total_steps = engine::resolve_pipeline(&config.pipeline).len();

    let final_job = match result {
        Ok(()) if cancel.load(Ordering::Relaxed) => job_state(
            &job_id,
            JobStatus::Cancelled,
            None,
            total_docs,
            total_docs,
            0,
            total_steps,
            0,
            None,
        ),
        Ok(()) => job_state(
            &job_id,
            JobStatus::Completed,
            None,
            total_docs,
            total_docs,
            total_steps,
            total_steps,
            100,
            None,
        ),
        Err(err) => job_state(
            &job_id,
            JobStatus::Failed,
            None,
            0,
            total_docs,
            0,
            total_steps,
            0,
            Some(err.to_string()),
        ),
    };
    jobs.write().await.insert(job_id.clone(), final_job);
    *res.pipeline.write().await = None;
}

#[tracing::instrument(level = "info", skip_all, fields(pages))]
async fn run_inner(
    res: &AppResources,
    req: &koharu_core::commands::ProcessRequest,
    cancel: &Arc<AtomicBool>,
    job_id: &str,
    jobs: &Jobs,
) -> anyhow::Result<()> {
    let page_ids: Vec<String> = match req.document_id.as_deref() {
        Some(id) => {
            res.storage.page(id).await?;
            vec![id.to_string()]
        }
        None => res.storage.page_ids().await,
    };
    let total_docs = page_ids.len();
    tracing::Span::current().record("pages", total_docs);
    if total_docs == 0 {
        return Ok(());
    }

    let config = res.config.read().await.clone();
    let selection = engine::resolve_pipeline(&config.pipeline);
    let skip_empty_pages = config.pipeline.skip_empty_pages;
    let detector_selection = selection.iter().try_fold(Vec::new(), |mut ids, &id| {
        if engine::Registry::find(id)?
            .produces
            .contains(&engine::Artifact::TextBlocks)
        {
            ids.push(id);
        }
        Ok::<_, anyhow::Error>(ids)
    })?;
    let infos: Vec<_> = selection
        .iter()
        .map(|id| engine::Registry::find(id))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let order = engine::build_order(&infos)?;
    let step_indices: HashMap<_, _> = order
        .into_iter()
        .enumerate()
        .map(|(step_idx, info_idx)| (infos[info_idx].id, step_idx))
        .collect();
    let total_steps = selection.len();

    for (doc_idx, page_id) in page_ids.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        res.storage
            .update_page(page_id, |page| {
                page.detected = false;
                page.text_blocks.clear();
                page.bubbles.clear();
                page.segment = None;
                page.inpainted = None;
                page.rendered = None;
            })
            .await?;

        let job_id = job_id.to_string();
        let jobs = jobs.clone();

        if skip_empty_pages && !detector_selection.is_empty() {
            engine::execute_pipeline(&detector_selection, res, page_id, cancel, |_, step_id| {
                let step_idx = step_indices[step_id];
                let pct = progress_percent(doc_idx, step_idx, total_docs, total_steps);
                let job = job_state(
                    &job_id,
                    JobStatus::Running,
                    Some(step_id.to_string()),
                    doc_idx + 1,
                    total_docs,
                    step_idx,
                    total_steps,
                    pct,
                    None,
                );
                let jobs = jobs.clone();
                async move {
                    jobs.write().await.insert(job.id.clone(), job);
                }
            })
            .await?;

            let page = res.storage.page(page_id).await?;
            if page.detected && page.text_blocks.is_empty() {
                let job = job_state(
                    &job_id,
                    JobStatus::Running,
                    Some("Skipped".to_string()),
                    doc_idx + 1,
                    total_docs,
                    total_steps,
                    total_steps,
                    progress_percent(doc_idx + 1, 0, total_docs, total_steps),
                    None,
                );
                jobs.write().await.insert(job.id.clone(), job);
                continue;
            }
        }

        engine::execute_pipeline(&selection, res, page_id, cancel, |_, step_id| {
            let step_idx = step_indices[step_id];
            let pct = progress_percent(doc_idx, step_idx, total_docs, total_steps);
            let job = job_state(
                &job_id,
                JobStatus::Running,
                Some(step_id.to_string()),
                doc_idx + 1,
                total_docs,
                step_idx,
                total_steps,
                pct,
                None,
            );
            let jobs = jobs.clone();
            async move {
                jobs.write().await.insert(job.id.clone(), job);
            }
        })
        .await?;
    }

    Ok(())
}

fn progress_percent(doc_idx: usize, step_idx: usize, total_docs: usize, total_steps: usize) -> u8 {
    if total_docs * total_steps > 0 {
        ((doc_idx * total_steps + step_idx) as f64 / (total_docs * total_steps) as f64 * 100.0)
            as u8
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
fn job_state(
    id: &str,
    status: JobStatus,
    step: Option<String>,
    doc: usize,
    total_docs: usize,
    step_idx: usize,
    total_steps: usize,
    pct: u8,
    error: Option<String>,
) -> JobState {
    JobState {
        id: id.to_string(),
        kind: "pipeline".to_string(),
        status,
        step,
        current_document: doc,
        total_documents: total_docs,
        current_step_index: step_idx,
        total_steps,
        overall_percent: pct,
        error,
    }
}
