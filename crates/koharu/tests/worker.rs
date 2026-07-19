use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use koharu_config::Config;
use koharu_pipeline::{Phase, Pipeline, PipelineConfig, PipelineEvent, WorkerState};
use koharu_scene::Session;
use koharu_translator::{OpenAiConfig, Providers, TranslationConfig};

#[tokio::test]
async fn model_process_reports_events_and_reads_shared_inputs() {
    let translation = TranslationConfig {
        model: Providers::OpenAi(OpenAiConfig::default()),
        ..TranslationConfig::default()
    };
    let pipeline = Pipeline::with_worker_executable(
        Config::memory(PipelineConfig {
            processors: Vec::new(),
        }),
        Config::memory(translation),
        env!("CARGO_BIN_EXE_koharu"),
    );
    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    commands.add_page("page", source_png()).unwrap();
    session.apply(commands).unwrap();

    let states = Arc::new(Mutex::new(Vec::new()));
    let input_bytes = Arc::new(Mutex::new(None));
    let states_sink = states.clone();
    let input_sink = input_bytes.clone();
    let events = Arc::new(move |event| match event {
        PipelineEvent::Worker(event) => states_sink.lock().unwrap().push(event.state),
        PipelineEvent::Measurement(measurement) => {
            *input_sink.lock().unwrap() = Some(measurement.input_bytes);
        }
        PipelineEvent::Progress(_) | PipelineEvent::Download(_) => {}
    });

    let report = pipeline
        .run(&mut session)
        .phase(Phase::Translation)
        .events(events)
        .execute()
        .await
        .unwrap();

    assert_eq!(report.processors, 1);
    assert!(report.revisions.is_empty());
    assert_eq!(report.measurements.len(), 1);
    assert!(report.measurements[0].input_bytes > 0);
    assert_eq!(
        states.lock().unwrap().as_slice(),
        [
            WorkerState::Spawned,
            WorkerState::Loading,
            WorkerState::Ready,
            WorkerState::Running,
        ]
    );
    assert!(input_bytes.lock().unwrap().is_some_and(|bytes| bytes > 0));
    pipeline.unload_all().await.unwrap();
}

fn source_png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, Rgba([255; 4])));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}
