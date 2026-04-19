//! `POST /pipelines` — start a pipeline run as a long-running operation.
//!
//! Returns an `operationId`. Progress + completion flow through SSE
//! (`JobStarted` / `JobProgress` / `JobFinished`). Cancellation goes to
//! `DELETE /operations/{id}`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axum::Json;
use axum::extract::State;
use koharu_app::pipeline::{self, PipelineRunOptions, PipelineSpec, ProgressTick, Scope};
use koharu_core::{
    AppEvent, JobFinishedEvent, JobStatus, JobSummary, PageId, PipelineProgress, PipelineStatus,
    Region,
};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::AppState;
use crate::error::{ApiError, ApiResult};
use crate::routes::operations::{register_cancel, unregister_cancel};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::default().routes(routes!(start_pipeline))
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartPipelineRequest {
    /// Engine ids (`inventory::submit!` ids) to run in order.
    pub steps: Vec<String>,
    /// `None` → whole project, `Some(pages)` → just those pages.
    #[serde(default)]
    pub pages: Option<Vec<PageId>>,
    /// Optional bounding-box hint for inpainter engines (repair-brush).
    #[serde(default)]
    pub region: Option<Region>,
    #[serde(default)]
    pub target_language: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub default_font: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartPipelineResponse {
    pub operation_id: String,
}

#[utoipa::path(
    post,
    path = "/pipelines",
    request_body = StartPipelineRequest,
    responses((status = 200, body = StartPipelineResponse))
)]
async fn start_pipeline(
    State(app): State<AppState>,
    Json(req): Json<StartPipelineRequest>,
) -> ApiResult<Json<StartPipelineResponse>> {
    let session = app
        .current_session()
        .ok_or_else(|| ApiError::bad_request("no project open"))?;
    // Validate every step resolves to a registered engine before spawning.
    for id in &req.steps {
        pipeline::Registry::find(id).map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    }
    let spec = PipelineSpec {
        scope: match req.pages {
            Some(pages) => Scope::Pages(pages),
            None => Scope::WholeProject,
        },
        steps: req.steps,
        options: PipelineRunOptions {
            target_language: req.target_language,
            system_prompt: req.system_prompt,
            default_font: req.default_font,
            region: req.region,
        },
    };

    let operation_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    register_cancel(operation_id.clone(), cancel.clone());
    app.jobs.insert(
        operation_id.clone(),
        JobSummary {
            id: operation_id.clone(),
            kind: "pipeline".to_string(),
            status: JobStatus::Running,
            error: None,
        },
    );
    app.bus.publish(AppEvent::JobStarted {
        id: operation_id.clone(),
        kind: "pipeline".to_string(),
    });

    // Detach the pipeline. Progress writes directly into the jobs registry;
    // clients observe via SSE.
    let app_c = app.clone();
    let session_c = session.clone();
    let op_id_c = operation_id.clone();
    let registry_c = app.registry.clone();
    let runtime_c = app.runtime.clone();
    let llm_c = app.llm.clone();
    let renderer_c = app.renderer.clone();
    let cpu = app.cpu_only();
    let progress_bus = app.bus.clone();
    let progress_op_id = operation_id.clone();
    let progress_sink: pipeline::ProgressSink = Arc::new(move |tick: ProgressTick| {
        progress_bus.publish(AppEvent::JobProgress(PipelineProgress {
            job_id: progress_op_id.clone(),
            status: PipelineStatus::Running,
            step: tick.step,
            current_page: tick.page_index,
            total_pages: tick.total_pages,
            current_step_index: tick.step_index,
            total_steps: tick.total_steps,
            overall_percent: tick.overall_percent,
        }));
    });
    tokio::spawn(async move {
        let result = pipeline::run(
            session_c,
            registry_c,
            runtime_c,
            cpu,
            llm_c,
            renderer_c,
            spec,
            cancel,
            Some(progress_sink),
        )
        .await;
        let (status, error) = match &result {
            Ok(()) => (JobStatus::Completed, None),
            Err(e) if e.to_string().contains("cancelled") => (JobStatus::Cancelled, None),
            Err(e) => (JobStatus::Failed, Some(format!("{e:#}"))),
        };
        app_c.jobs.insert(
            op_id_c.clone(),
            JobSummary {
                id: op_id_c.clone(),
                kind: "pipeline".to_string(),
                status,
                error: error.clone(),
            },
        );
        app_c.bus.publish(AppEvent::JobFinished(JobFinishedEvent {
            id: op_id_c.clone(),
            status,
            error,
        }));
        unregister_cancel(&op_id_c);
    });

    Ok(Json(StartPipelineResponse { operation_id }))
}
