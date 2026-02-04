use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use koharu_ml::llm::ModelId;
use koharu_renderer::renderer::TextShaderEffect;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tokio::sync::broadcast;

use crate::app::AppResources;

/// Steps in the processing pipeline.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PipelineStep {
    Detect,
    Ocr,
    Inpaint,
    LlmGenerate,
    Render,
}

impl PipelineStep {
    pub const ALL: &[PipelineStep] = &[
        PipelineStep::Detect,
        PipelineStep::Ocr,
        PipelineStep::Inpaint,
        PipelineStep::LlmGenerate,
        PipelineStep::Render,
    ];
}

/// Status of the pipeline.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PipelineStatus {
    Running,
    Completed,
    Cancelled,
    Failed(String),
}

/// SSE event payload sent to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineProgress {
    pub status: PipelineStatus,
    pub step: Option<PipelineStep>,
    pub current_document: usize,
    pub total_documents: usize,
    pub current_step_index: usize,
    pub total_steps: usize,
    pub overall_percent: u8,
}

/// Request payload for the auto-process endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRequest {
    /// None means all documents; Some(i) means single document.
    pub index: Option<usize>,
    /// LLM model id to use (will load if not already loaded).
    pub llm_model_id: Option<String>,
    /// Target language for translation.
    pub language: Option<String>,
    /// Shader effect for rendering.
    pub shader_effect: Option<TextShaderEffect>,
}

/// Handle to a running pipeline, used for cancellation.
pub struct PipelineHandle {
    pub cancel: Arc<AtomicBool>,
}

// Global broadcast channel for pipeline progress (mirrors download.rs pattern).
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

/// Run the processing pipeline. Called from a spawned task.
pub async fn run_pipeline(
    resources: AppResources,
    request: ProcessRequest,
    cancel: Arc<AtomicBool>,
) {
    let result = run_pipeline_inner(&resources, &request, &cancel).await;

    let total_docs = match &request.index {
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
        Err(e) => {
            tracing::error!("Pipeline failed: {e:#}");
            emit(PipelineProgress {
                status: PipelineStatus::Failed(e.to_string()),
                step: None,
                current_document: 0,
                total_documents: total_docs,
                current_step_index: 0,
                total_steps: PipelineStep::ALL.len(),
                overall_percent: 0,
            });
        }
    }

    // Clear the pipeline handle
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
            Some(i) if i >= len => {
                anyhow::bail!("Document index {} out of range (have {})", i, len);
            }
            Some(_) => 1,
            None => len,
        }
    };

    if total_docs == 0 {
        return Ok(());
    }

    // Ensure LLM is loaded
    if let Some(model_id) = &req.llm_model_id {
        if !res.llm.ready().await {
            let id = ModelId::from_str(model_id)?;
            res.llm.load(id).await;
            // Poll until ready (with timeout)
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

    if let Some(locale) = req.language.as_ref() {
        koharu_ml::set_locale(locale.clone());
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

            // Give the runtime a chance to flush the SSE event before blocking ML work
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;

            // Snapshot → process → write back
            let mut snapshot =
                {
                    let guard = res.state.read().await;
                    guard.documents.get(doc_index).cloned().ok_or_else(|| {
                        anyhow::anyhow!("Document not found at index {}", doc_index)
                    })?
                };

            match step {
                PipelineStep::Detect => res.ml.detect(&mut snapshot).await?,
                PipelineStep::Ocr => res.ml.ocr(&mut snapshot).await?,
                PipelineStep::Inpaint => res.ml.inpaint(&mut snapshot).await?,
                PipelineStep::LlmGenerate => {
                    res.llm.generate(&mut snapshot).await?;
                }
                PipelineStep::Render => {
                    res.renderer.render(
                        &mut snapshot,
                        None,
                        req.shader_effect.unwrap_or_default(),
                    )?;
                }
            }

            let mut guard = res.state.write().await;
            let document = guard
                .documents
                .get_mut(doc_index)
                .ok_or_else(|| anyhow::anyhow!("Document not found at index {}", doc_index))?;
            *document = snapshot;
        }
    }

    Ok(())
}
