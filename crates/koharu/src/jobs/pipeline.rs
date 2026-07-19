use std::sync::Arc;

use anyhow::Context as _;
use koharu_desktop::DesktopHandle;
use koharu_pipeline::{CancellationToken, Pipeline, PipelineEvent};
use koharu_scene::Session;

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
        force,
    } = request;
    let mut session =
        match Session::open(&path).with_context(|| format!("failed to open {}", path.display())) {
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
    let event_handle = desktop.clone();
    let events = Arc::new(move |event| handle_event(&event_handle, id, event));
    let run = pipeline
        .run(&mut session)
        .scope(scope)
        .target(target)
        .force(force)
        .cancellation(cancellation.clone())
        .events(events);
    let outcome = match run.execute().await {
        Ok(report) => JobOutcome {
            revisions: report.revisions,
            ..JobOutcome::default()
        },
        Err(error) => JobOutcome {
            revisions: error.committed_revisions.clone(),
            error: (!cancellation.is_cancelled()).then(|| error.to_string()),
            ..JobOutcome::default()
        },
    };
    finish_job(&desktop, id, &cancellation, outcome);
}

fn handle_event(desktop: &DesktopHandle<NativeEvent>, job: RequestId, event: PipelineEvent) {
    match event {
        PipelineEvent::Progress(progress) => {
            let _ = desktop.send_event(NativeEvent::ProjectAdvanced { job });
            let _ = desktop.send_event(NativeEvent::PipelineProgress { job, progress });
        }
        PipelineEvent::Download(event) => {
            let _ = desktop.send_event(NativeEvent::Download(event));
        }
        PipelineEvent::Worker(event) => {
            tracing::info!(
                phase = %event.phase,
                model = event.model,
                generation = event.generation,
                state = ?event.state,
                detail = event.detail,
                "model worker state changed"
            );
        }
        PipelineEvent::Measurement(measurement) => {
            tracing::info!(
                phase = %measurement.phase,
                model = measurement.model,
                generation = measurement.generation,
                cold = measurement.cold,
                load_ms = measurement.load.map(|value| value.as_secs_f64() * 1000.0),
                input_transfer_ms = measurement.input_transfer.as_secs_f64() * 1000.0,
                processor_ms = measurement.processor.as_secs_f64() * 1000.0,
                output_transfer_ms = measurement.output_transfer.as_secs_f64() * 1000.0,
                round_trip_ms = measurement.round_trip.as_secs_f64() * 1000.0,
                input_bytes = measurement.input_bytes,
                output_bytes = measurement.output_bytes,
                control_request_bytes = measurement.control_request_bytes,
                control_response_bytes = measurement.control_response_bytes,
                "model worker measurement"
            );
        }
    }
}
