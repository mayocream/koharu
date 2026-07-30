use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use koharu_desktop::DesktopHandle;
use koharu_pipeline::{Committer, Pipeline, Progress, Request, RunStatus, StageOutput, StopToken};
use koharu_scene::{Revision, SceneSession, SceneSnapshot};

use super::{JobOutcome, NativeEvent, PipelineRequest, finish_job};
use crate::protocol::RequestId;

pub(super) async fn run(
    pipeline: &Pipeline,
    request: PipelineRequest,
    stop: StopToken,
    desktop: DesktopHandle<NativeEvent>,
) {
    let PipelineRequest {
        id,
        path,
        scope,
        operation,
    } = request;
    let mut session = match SceneSession::open(&path)
        .with_context(|| format!("failed to open {}", path.display()))
    {
        Ok(session) => session,
        Err(error) => {
            finish_job(
                &desktop,
                id,
                JobOutcome {
                    stopped: stop.stopped(),
                    error: Some(error.to_string()),
                    ..JobOutcome::default()
                },
            );
            return;
        }
    };
    let progress = Arc::new(Mutex::new(ProgressState::default()));
    let event_handle = desktop.clone();
    let event_progress = progress.clone();
    let events = Arc::new(move |event| {
        handle_progress(&event_handle, id, &event_progress, event);
    });
    let request = Request {
        operation,
        scope,
        stop,
        progress: Some(events),
    };
    let snapshot = session.snapshot();
    let mut revisions = Vec::new();
    let result = {
        let mut committer = SessionCommitter {
            session: &mut session,
            revisions: &mut revisions,
            desktop: &desktop,
            job: id,
        };
        pipeline.execute(snapshot, request, &mut committer).await
    };
    let outcome = match result {
        Ok(report) => JobOutcome {
            revisions,
            stopped: report.status == RunStatus::Stopped,
            ..JobOutcome::default()
        },
        Err(error) => {
            tracing::error!(stage = ?error.stage, %error, "pipeline execution failed");
            JobOutcome {
                revisions,
                error: Some(error.to_string()),
                ..JobOutcome::default()
            }
        }
    };
    finish_job(&desktop, id, outcome);
}

struct SessionCommitter<'a> {
    session: &'a mut SceneSession,
    revisions: &'a mut Vec<Revision>,
    desktop: &'a DesktopHandle<NativeEvent>,
    job: RequestId,
}

#[async_trait::async_trait]
impl Committer for SessionCommitter<'_> {
    async fn commit(&mut self, output: StageOutput) -> anyhow::Result<SceneSnapshot> {
        let commit = self.session.commit(output.patch)?;
        if commit.changes.to != commit.changes.from {
            self.revisions.push(commit.revision);
            let _ = self
                .desktop
                .send_event(NativeEvent::ProjectAdvanced { job: self.job });
        }
        Ok(commit.snapshot)
    }
}

#[derive(Default)]
struct ProgressState {
    completed: usize,
    total: usize,
}

fn handle_progress(
    desktop: &DesktopHandle<NativeEvent>,
    job: RequestId,
    progress: &Mutex<ProgressState>,
    event: Progress,
) {
    let update = match event {
        Progress::Started { pages, stages } => {
            let mut progress = progress.lock().unwrap_or_else(|error| error.into_inner());
            progress.completed = 0;
            progress.total = pages.len().saturating_mul(stages.len());
            Some((0, progress.total, None, None, None))
        }
        Progress::Loading { page, stage, model } => {
            let progress = progress.lock().unwrap_or_else(|error| error.into_inner());
            Some((
                progress.completed,
                progress.total,
                Some(page),
                Some(stage),
                Some(model),
            ))
        }
        Progress::Finished {
            page, stage, model, ..
        } => {
            let mut progress = progress.lock().unwrap_or_else(|error| error.into_inner());
            progress.completed = progress.completed.saturating_add(1).min(progress.total);
            Some((
                progress.completed,
                progress.total,
                Some(page),
                Some(stage),
                Some(model),
            ))
        }
        Progress::Skipped { page, stage } => {
            let mut progress = progress.lock().unwrap_or_else(|error| error.into_inner());
            progress.completed = progress.completed.saturating_add(1).min(progress.total);
            Some((
                progress.completed,
                progress.total,
                Some(page),
                Some(stage),
                None,
            ))
        }
        Progress::Running { .. } => None,
    };
    if let Some((completed, total, page, stage, model)) = update {
        let _ = desktop.send_event(NativeEvent::PipelineProgress {
            job,
            completed,
            total,
            page,
            stage,
            model,
        });
    }
}
