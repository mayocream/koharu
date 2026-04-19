//! Server → client push events.
//!
//! Delivered over the SSE stream (`GET /events`). Scoped to long-running
//! processes — pipeline jobs and runtime downloads — plus the LLM
//! lifecycle (loading a multi-GB model is minutes of work), and a
//! `Snapshot` replay on (re)connect. Project / scene / config state is
//! still caller-driven: the HTTP client that triggered a change re-fetches
//! the relevant resource.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::protocol::LlmTarget;

// ---------------------------------------------------------------------------
// AppEvent
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum AppEvent {
    // Pipeline jobs.
    JobStarted { id: String, kind: String },
    JobProgress(PipelineProgress),
    JobFinished(JobFinishedEvent),

    // Runtime library / model downloads.
    DownloadProgress(DownloadProgress),

    // LLM lifecycle. Loading is a long-running operation; every state
    // transition fires an event so clients can refetch `GET /llm/current`
    // and show the right indicator.
    //
    // - `LlmLoading`  — background load has started for `target`.
    // - `LlmLoaded`   — model is on the GPU and ready for inference.
    // - `LlmFailed`   — load failed; see `GET /llm/current` for the reason.
    // - `LlmUnloaded` — model released.
    LlmLoading { target: LlmTarget },
    LlmLoaded { target: LlmTarget },
    LlmFailed { target: Option<LlmTarget> },
    LlmUnloaded,

    // (Re)connect replay so the client can seed in-flight state.
    Snapshot(SnapshotEvent),
}

// ---------------------------------------------------------------------------
// Pipeline progress
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
#[derive(strum::Display)]
#[strum(serialize_all = "snake_case")]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, ToSchema)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PipelineStatus {
    Running,
    Completed,
    Cancelled,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineProgress {
    pub job_id: String,
    pub status: PipelineStatus,
    pub step: Option<PipelineStep>,
    pub current_page: usize,
    pub total_pages: usize,
    pub current_step_index: usize,
    pub total_steps: usize,
    pub overall_percent: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Completed,
    CompletedWithErrors,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    pub id: String,
    pub kind: String,
    pub status: JobStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobFinishedEvent {
    pub id: String,
    pub status: JobStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, ToSchema)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DownloadStatus {
    Started,
    Downloading,
    Completed,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: String,
    pub filename: String,
    pub downloaded: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    pub status: DownloadStatus,
}

// ---------------------------------------------------------------------------
// Project / snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    /// Stable identifier — the `.khrproj` directory basename (without the
    /// extension). Clients address projects by this.
    pub id: String,
    pub name: String,
    /// Absolute filesystem path. Informational; clients never need to pass
    /// it back in — they use `id`.
    pub path: String,
    /// Last modification time of the project directory on disk (ms since
    /// UNIX epoch). Used for "recent projects" ordering.
    #[serde(default)]
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEvent {
    pub jobs: Vec<JobSummary>,
    pub downloads: Vec<DownloadProgress>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_progress_round_trips() {
        let value = DownloadProgress {
            id: "model-x".into(),
            filename: "model.bin".into(),
            downloaded: 123,
            total: Some(456),
            status: DownloadStatus::Downloading,
        };
        let encoded = serde_json::to_string(&value).expect("serialize");
        let _: DownloadProgress = serde_json::from_str(&encoded).expect("deserialize");
    }

    #[test]
    fn pipeline_progress_round_trips() {
        let value = PipelineProgress {
            job_id: "j".into(),
            status: PipelineStatus::Running,
            step: Some(PipelineStep::Inpaint),
            current_page: 1,
            total_pages: 3,
            current_step_index: 2,
            total_steps: 5,
            overall_percent: 40,
        };
        let encoded = serde_json::to_string(&value).expect("serialize");
        let _: PipelineProgress = serde_json::from_str(&encoded).expect("deserialize");
    }
}
