use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use anyhow::Context as _;
use koharu_desktop::DesktopHandle;
use koharu_pipeline::{CancellationToken, Pipeline, PipelineEvent, Stage};
use koharu_scene::SceneSession;

use super::{JobOutcome, NativeEvent, PipelineRequest, finish_job};
use crate::protocol::RequestId;

pub(super) async fn run(
    pipeline: &Pipeline,
    request: PipelineRequest,
    cancellation: CancellationToken,
    desktop: DesktopHandle<NativeEvent>,
) {
    let PipelineRequest {
        id,
        path,
        scope,
        target,
    } = request;
    let mut session = match SceneSession::open(&path)
        .with_context(|| format!("failed to open {}", path.display()))
    {
        Ok(session) => session,
        Err(error) => {
            finish_job(
                &desktop,
                id,
                &cancellation,
                JobOutcome {
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
        handle_event(&event_handle, id, &event_progress, event);
    });
    let run = pipeline
        .run(session.snapshot())
        .scope(scope)
        .target(target)
        .cancellation(cancellation.clone())
        .events(events);
    let outcome = match run.execute().await {
        Ok(report) => match session.commit(report.patch) {
            Ok(commit) => {
                let _ = desktop.send_event(NativeEvent::ProjectAdvanced { job: id });
                JobOutcome {
                    revisions: vec![commit.revision],
                    ..JobOutcome::default()
                }
            }
            Err(error) => JobOutcome {
                error: Some(error.to_string()),
                ..JobOutcome::default()
            },
        },
        Err(error) => JobOutcome {
            error: (!cancellation.is_cancelled()).then(|| error.to_string()),
            ..JobOutcome::default()
        },
    };
    finish_job(&desktop, id, &cancellation, outcome);
}

#[derive(Default)]
struct ProgressState {
    completed: usize,
    total: usize,
    models: BTreeMap<Stage, String>,
}

fn handle_event(
    desktop: &DesktopHandle<NativeEvent>,
    job: RequestId,
    progress: &Mutex<ProgressState>,
    event: PipelineEvent,
) {
    let update = match event {
        PipelineEvent::RunStarted { stages, .. } => {
            let mut progress = progress.lock().unwrap_or_else(|error| error.into_inner());
            progress.completed = 0;
            progress.total = stages.len();
            Some((0, progress.total, None, None))
        }
        PipelineEvent::ModelLoadStarted { stage, model, .. } => {
            let mut progress = progress.lock().unwrap_or_else(|error| error.into_inner());
            progress.models.insert(stage, model.clone());
            Some((progress.completed, progress.total, Some(stage), Some(model)))
        }
        PipelineEvent::StageFinished { stage, .. } => {
            let mut progress = progress.lock().unwrap_or_else(|error| error.into_inner());
            progress.completed = progress.completed.saturating_add(1).min(progress.total);
            Some((
                progress.completed,
                progress.total,
                Some(stage),
                progress.models.get(&stage).cloned(),
            ))
        }
        PipelineEvent::RunFailed { stage, message, .. } => {
            tracing::error!(stage = ?stage, %message, "pipeline run failed");
            None
        }
        _ => None,
    };
    if let Some((completed, total, stage, model)) = update {
        let _ = desktop.send_event(NativeEvent::PipelineProgress {
            job,
            completed,
            total,
            stage,
            model,
        });
    }
}
