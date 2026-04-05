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
    /// Target locale for translation (from [`ProcessRequest::language`](koharu_core::commands::ProcessRequest)).
    pub target_language: Option<String>,
}

#[derive(Debug, Default)]
struct BatchReport {
    total_docs: usize,
    processed_docs: usize,
    page_errors: Vec<PageError>,
}

#[derive(Debug)]
struct PageError {
    page_id: String,
    message: String,
}

impl BatchReport {
    fn new(total_docs: usize) -> Self {
        Self {
            total_docs,
            ..Self::default()
        }
    }

    fn successful_docs(&self) -> usize {
        self.processed_docs.saturating_sub(self.page_errors.len())
    }

    fn push_page_error(&mut self, page_id: &str, message: String) {
        self.page_errors.push(PageError {
            page_id: page_id.to_string(),
            message,
        });
    }

    fn error_summary(&self) -> Option<String> {
        if self.page_errors.is_empty() {
            return None;
        }

        let preview = self
            .page_errors
            .iter()
            .take(3)
            .map(|error| {
                let message = error.message.replace('\n', " ");
                format!("{}: {}", error.page_id, message)
            })
            .collect::<Vec<_>>();

        let mut summary = format!(
            "{} of {} page{} failed:\n{}",
            self.page_errors.len(),
            self.total_docs,
            if self.total_docs == 1 { "" } else { "s" },
            preview.join("\n")
        );

        let omitted = self.page_errors.len().saturating_sub(preview.len());
        if omitted > 0 {
            summary.push_str(&format!("\n...and {omitted} more"));
        }

        Some(summary)
    }
}

pub async fn process(
    state: AppResources,
    payload: koharu_core::commands::ProcessRequest,
    jobs: Jobs,
) -> anyhow::Result<String> {
    let total_docs = match payload.document_id.as_deref() {
        Some(_) => 1,
        None => state.storage.page_count().await,
    };
    let config = state.config.read().await.clone();
    let total_steps = engine::resolve_pipeline(&config.pipeline).len();

    let job_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.pipeline.write().await;
        if guard.is_some() {
            anyhow::bail!("A processing pipeline is already running");
        }
        *guard = Some(PipelineHandle {
            id: job_id.clone(),
            cancel: cancel.clone(),
            target_language: payload.language.clone(),
        });
    }

    let initial_job = job_state(
        &job_id,
        JobStatus::Running,
        None,
        0,
        total_docs,
        0,
        total_steps,
        0,
        None,
    );
    jobs.write().await.insert(job_id.clone(), initial_job);

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
        Ok(report) if cancel.load(Ordering::Relaxed) => job_state(
            &job_id,
            JobStatus::Cancelled,
            None,
            report.processed_docs.min(total_docs),
            total_docs,
            0,
            total_steps,
            0,
            None,
        ),
        Ok(report) if report.page_errors.is_empty() => job_state(
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
        Ok(report) if report.successful_docs() > 0 => job_state(
            &job_id,
            JobStatus::CompletedWithErrors,
            None,
            total_docs,
            total_docs,
            total_steps,
            total_steps,
            100,
            report.error_summary(),
        ),
        Ok(report) => job_state(
            &job_id,
            JobStatus::Failed,
            None,
            report.processed_docs.min(total_docs),
            total_docs,
            total_steps,
            total_steps,
            100,
            report.error_summary(),
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
) -> anyhow::Result<BatchReport> {
    let page_ids: Vec<String> = match req.document_id.as_deref() {
        Some(id) => {
            res.storage.page(id).await?;
            vec![id.to_string()]
        }
        None => res.storage.page_ids().await,
    };
    let total_docs = page_ids.len();
    let mut report = BatchReport::new(total_docs);
    tracing::Span::current().record("pages", total_docs);
    if total_docs == 0 {
        return Ok(report);
    }

    let config = res.config.read().await.clone();
    let selection = engine::resolve_pipeline(&config.pipeline);
    let total_steps = selection.len();
    let run_options = engine::PipelineRunOptions::from_process_request(req);

    if selection.contains(&"llm")
        && let Some(llm) = req.llm.as_ref()
    {
        crate::llm::llm_load(
            res.clone(),
            koharu_core::LlmLoadRequest {
                target: llm.target.clone(),
                options: llm.options.clone(),
            },
        )
        .await?;
    }

    for (doc_idx, page_id) in page_ids.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(report);
        }

        let job_id = job_id.to_string();
        let jobs = jobs.clone();

        match engine::execute_pipeline(
            &selection,
            res,
            page_id,
            cancel,
            &run_options,
            |step_idx, step_id| {
                let pct = if total_docs * total_steps > 0 {
                    ((doc_idx * total_steps + step_idx) as f64 / (total_docs * total_steps) as f64
                        * 100.0) as u8
                } else {
                    0
                };
                let job = job_state(
                    &job_id,
                    JobStatus::Running,
                    Some(step_id.to_string()),
                    doc_idx,
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
            },
        )
        .await
        {
            Ok(()) => {
                report.processed_docs = doc_idx + 1;
            }
            Err(err) => {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(report);
                }

                report.processed_docs = doc_idx + 1;
                tracing::error!(page_id, error = %err, "page pipeline failed");
                report.push_page_error(page_id, err.to_string());
            }
        }
    }

    Ok(report)
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

#[cfg(test)]
mod tests {
    use super::BatchReport;

    #[test]
    fn batch_report_summarizes_page_errors() {
        let mut report = BatchReport::new(4);
        report.processed_docs = 4;
        report.push_page_error("page-1", "step 'llm' failed".to_string());
        report.push_page_error("page-2", "step 'render' failed\nwith details".to_string());
        report.push_page_error("page-3", "step 'ocr' failed".to_string());
        report.push_page_error("page-4", "step 'font' failed".to_string());

        let summary = report.error_summary().expect("summary");
        assert!(summary.contains("4 of 4 pages failed"));
        assert!(summary.contains("page-1: step 'llm' failed"));
        assert!(summary.contains("page-2: step 'render' failed with details"));
        assert!(summary.contains("...and 1 more"));
    }
}
