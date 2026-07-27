use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::Result;
use async_trait::async_trait;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use koharu_scene::{PageAsset, Session};
use koharu_translator::TranslationConfig;
use strum::IntoEnumIterator;

use super::*;

#[test]
fn strum_owns_phase_iteration_display_and_parsing() {
    assert_eq!(
        Phase::iter().collect::<Vec<_>>(),
        [
            Phase::Detection,
            Phase::Ocr,
            Phase::Translation,
            Phase::Typography,
            Phase::Inpainting,
        ]
    );
    assert_eq!(Phase::Typography.to_string(), "typography");
    assert_eq!("translate".parse::<Phase>().unwrap(), Phase::Translation);
}

struct FakeFactory {
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    active_accelerator: Arc<AtomicUsize>,
    maximum_accelerator: Arc<AtomicUsize>,
    detection_writes_clean: bool,
}

#[async_trait]
impl ProcessorFactory for FakeFactory {
    async fn create(&self, node: &ConfiguredNode, _device: Device) -> Result<Box<dyn Processor>> {
        Ok(Box::new(FakeProcessor {
            node: node.clone(),
            active: self.active.clone(),
            maximum: self.maximum.clone(),
            active_accelerator: self.active_accelerator.clone(),
            maximum_accelerator: self.maximum_accelerator.clone(),
            detection_writes_clean: self.detection_writes_clean,
        }))
    }
}

struct FakeProcessor {
    node: ConfiguredNode,
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    active_accelerator: Arc<AtomicUsize>,
    maximum_accelerator: Arc<AtomicUsize>,
    detection_writes_clean: bool,
}

#[async_trait]
impl Processor for FakeProcessor {
    async fn run(&mut self, context: &Context) -> Result<Commands> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        if self.node.uses_accelerator() {
            let active = self.active_accelerator.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum_accelerator.fetch_max(active, Ordering::SeqCst);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        if self.node.uses_accelerator() {
            self.active_accelerator.fetch_sub(1, Ordering::SeqCst);
        }
        self.active.fetch_sub(1, Ordering::SeqCst);

        let mut commands = context.commands();
        if self.detection_writes_clean && matches!(self.node, ConfiguredNode::Detection(_)) {
            for page in context.pages() {
                commands.set_asset(page.id, PageAsset::Clean, Some(source_png()))?;
            }
        }
        Ok(commands)
    }
}

#[tokio::test]
async fn all_five_nodes_run_in_fixed_topological_waves() {
    let (pipeline, maximum, _) = fake_pipeline(false);
    let mut session = session();

    let report = pipeline.run(&mut session).execute().await.unwrap();

    assert_eq!(report.processors, 5);
    assert_eq!(maximum.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn typography_runs_after_detection() {
    let (pipeline, _, _) = fake_pipeline(false);
    let mut session = session();

    let report = pipeline
        .run(&mut session)
        .phase(Phase::Typography)
        .execute()
        .await
        .unwrap();

    assert_eq!(report.processors, 2);
}

#[tokio::test]
async fn translation_runs_after_detection_and_ocr() {
    let (pipeline, _, _) = fake_pipeline(false);
    let mut session = session();

    let report = pipeline
        .run(&mut session)
        .phase(Phase::Translation)
        .execute()
        .await
        .unwrap();

    assert_eq!(report.processors, 3);
}

#[tokio::test]
async fn accelerator_processors_are_serialized() {
    let (mut pipeline, _, maximum_accelerator) = fake_pipeline(false);
    pipeline.device = Device::cuda(0);
    let mut session = session();

    pipeline.run(&mut session).execute().await.unwrap();

    assert_eq!(maximum_accelerator.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_stops_before_a_wave_commits() {
    let (pipeline, _, _) = fake_pipeline(false);
    let mut session = session();
    let cancellation = CancellationToken::default();
    cancellation.cancel();

    let error = pipeline
        .run(&mut session)
        .cancellation(cancellation)
        .execute()
        .await
        .unwrap_err();

    assert!(error.committed_revisions.is_empty());
}

#[tokio::test]
async fn pipeline_does_not_validate_processor_command_ownership() {
    let (pipeline, _, _) = fake_pipeline(true);
    let mut session = session();

    let report = pipeline
        .run(&mut session)
        .phase(Phase::Detection)
        .execute()
        .await
        .unwrap();

    assert_eq!(report.processors, 1);
    assert!(session.project().pages[0].assets.clean.is_some());
}

fn fake_pipeline(detection_writes_clean: bool) -> (Pipeline, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let active_accelerator = Arc::new(AtomicUsize::new(0));
    let maximum_accelerator = Arc::new(AtomicUsize::new(0));
    let mut pipeline = Pipeline::with_factory(
        Config::memory(PipelineConfig::default()),
        Config::memory(TranslationConfig::default()),
        Arc::new(FakeFactory {
            active,
            maximum: maximum.clone(),
            active_accelerator,
            maximum_accelerator: maximum_accelerator.clone(),
            detection_writes_clean,
        }),
    );
    pipeline.device = Device::cpu();
    (pipeline, maximum, maximum_accelerator)
}

fn session() -> Session {
    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    commands.add_page("page", source_png()).unwrap();
    session.apply(commands).unwrap();
    session
}

fn source_png() -> Arc<[u8]> {
    let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([255; 3])));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    Arc::from(bytes.into_inner())
}
