use std::{
    collections::HashMap,
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::Result;
use async_trait::async_trait;
use image::{DynamicImage, GrayImage, ImageFormat, Luma, Rgb, RgbImage};
use koharu_translator::TranslationConfig;

use super::*;

struct FakeFactory {
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    active_accelerator: Arc<AtomicUsize>,
    maximum_accelerator: Arc<AtomicUsize>,
    write_masks: bool,
}

#[async_trait]
impl ProcessorFactory for FakeFactory {
    async fn create(&self, model: &ConfiguredModel, _device: Device) -> Result<Box<dyn Processor>> {
        Ok(Box::new(FakeProcessor {
            model: model.clone(),
            active: self.active.clone(),
            maximum: self.maximum.clone(),
            active_accelerator: self.active_accelerator.clone(),
            maximum_accelerator: self.maximum_accelerator.clone(),
            write_masks: self.write_masks,
        }))
    }
}

struct FakeProcessor {
    model: ConfiguredModel,
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    active_accelerator: Arc<AtomicUsize>,
    maximum_accelerator: Arc<AtomicUsize>,
    write_masks: bool,
}

#[async_trait]
impl Processor for FakeProcessor {
    fn name(&self) -> &'static str {
        self.model.name()
    }

    fn inputs(&self) -> &'static [Artifact] {
        self.model.inputs()
    }

    fn outputs(&self) -> &'static [Artifact] {
        self.model.outputs()
    }

    async fn run(&mut self, context: &Context) -> Result<Commands> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        if self.model.uses_accelerator() {
            let active = self.active_accelerator.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum_accelerator.fetch_max(active, Ordering::SeqCst);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        if self.model.uses_accelerator() {
            self.active_accelerator.fetch_sub(1, Ordering::SeqCst);
        }
        self.active.fetch_sub(1, Ordering::SeqCst);

        let mut commands = context.commands();
        if self.write_masks {
            let asset = if self.model.outputs().contains(&Artifact::TextMask) {
                Some(PageAsset::TextMask)
            } else if self.model.outputs().contains(&Artifact::BubbleMask) {
                Some(PageAsset::BubbleMask)
            } else {
                None
            };
            if let Some(asset) = asset {
                for page in context.pages() {
                    commands.set_asset(
                        page.id,
                        asset,
                        Some(mask_png(page.size.width, page.size.height)?),
                    )?;
                }
            }
        }
        Ok(commands)
    }
}

#[tokio::test]
async fn independent_processors_run_together() {
    let (pipeline, maximum, _) = fake_pipeline(false);
    let mut session = session();

    let report = pipeline.run(&mut session).execute().await.unwrap();

    assert_eq!(report.processors, 9);
    assert!(report.revisions.is_empty());
    assert_eq!(maximum.load(Ordering::SeqCst), 3);
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
async fn one_wave_merges_into_one_scene_revision() {
    let (pipeline, _, _) = fake_pipeline_with_config(
        true,
        PipelineConfig {
            processors: vec![
                ProcessorConfig::SpeechBubbleYoloV8m(Default::default()),
                ProcessorConfig::MaskFusion(Default::default()),
            ],
        },
    );
    let mut session = session();
    let before = session.revision();

    let report = pipeline
        .run(&mut session)
        .phase(Phase::Segmentation)
        .execute()
        .await
        .unwrap();

    assert_eq!(report.revisions.len(), 1);
    assert_eq!(session.revision().get(), before.get() + 1);
    let page = &session.project().pages[0];
    assert!(page.assets.text_mask.is_some());
}

#[tokio::test]
async fn cancellation_stops_before_a_wave_commits() {
    let (pipeline, _, _) = fake_pipeline(true);
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
async fn fresh_dependencies_are_skipped_when_a_phase_is_rerun() {
    let (pipeline, _, _) = ocr_pipeline(false);
    let mut session = session();
    pipeline.run(&mut session).execute().await.unwrap();

    let report = pipeline
        .run(&mut session)
        .phase(Phase::Ocr)
        .execute()
        .await
        .unwrap();

    assert_eq!(report.processors, 1);
    assert_eq!(report.skipped, 1);
}

#[tokio::test]
async fn ensure_mode_skips_a_fresh_target() {
    let (pipeline, _, _) = ocr_pipeline(false);
    let mut session = session();
    pipeline.run(&mut session).execute().await.unwrap();

    let report = pipeline
        .run(&mut session)
        .phase(Phase::Ocr)
        .force(Force::None)
        .execute()
        .await
        .unwrap();

    assert_eq!(report.processors, 0);
    assert_eq!(report.skipped, 2);
}

#[tokio::test]
async fn changing_an_unrequested_output_port_does_not_stale_other_outputs() {
    let (pipeline, _, _) = ocr_pipeline(false);
    let mut session = session();
    pipeline.run(&mut session).execute().await.unwrap();
    let page = session.project().pages[0].id;
    let mut edit = session.commands();
    edit.set_asset(page, PageAsset::TextMask, Some(mask_png(8, 8).unwrap()))
        .unwrap();
    session.apply(edit).unwrap();

    let report = pipeline
        .run(&mut session)
        .phase(Phase::Ocr)
        .execute()
        .await
        .unwrap();

    assert_eq!(report.processors, 1);
    assert_eq!(report.skipped, 1);
}

#[test]
fn one_wave_context_reuses_one_shared_arena() {
    let session = session();
    let context = capture(
        &session,
        &RunRequest::default(),
        &mut HashMap::new(),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();

    let first = context.shared_snapshot(directory.path()).unwrap();
    let second = context.shared_snapshot(directory.path()).unwrap();

    assert!(Arc::ptr_eq(&first, &second));
    let arena = koharu_worker::MappedArena::open(
        first.descriptor.as_ref().unwrap(),
        directory.path(),
        false,
    )
    .unwrap();
    for (id, slice) in &first.blobs {
        assert_eq!(
            arena.slice(*slice).unwrap().as_ref(),
            session.read_blob(*id).unwrap().as_ref()
        );
    }
}

#[test]
fn detector_can_attach_metadata_to_a_new_text() {
    let session = session();
    let request = RunRequest::default();
    let context = capture(
        &session,
        &request,
        &mut HashMap::new(),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .unwrap();
    let page = context.pages()[0].id;
    let mut commands = context.commands();
    let element = commands.add_text(page, Frame::new(1.0, 1.0, 4.0, 4.0));
    commands.push(Command::EditElement {
        page,
        element,
        edit: ElementChange::Source(Some(koharu_scene::SourceText {
            text: String::new(),
            language: None,
            direction: koharu_scene::TextDirection::Auto,
            confidence: None,
            lines: Vec::new(),
        })),
    });
    let model = ConfiguredModel::Processor(ProcessorConfig::PPDocLayoutV3(Default::default()));

    validate_commands(&model, &context, &commands).unwrap();
}

#[test]
fn new_detection_invalidates_an_old_mask_and_clean_image() {
    let mut session = session();
    let page = session.project().pages[0].id;
    let mut setup = session.commands();
    setup
        .set_asset(page, PageAsset::TextMask, Some(mask_png(8, 8).unwrap()))
        .unwrap();
    setup
        .set_asset(page, PageAsset::Clean, Some(source_png()))
        .unwrap();
    session.apply(setup).unwrap();
    let context = capture(
        &session,
        &RunRequest::default(),
        &mut HashMap::new(),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .unwrap();
    let mut commands = context.commands();
    commands.add_text(page, Frame::new(1.0, 1.0, 4.0, 4.0));

    add_invalidations(&context, &mut commands);

    for asset in [PageAsset::TextMask, PageAsset::Clean] {
        assert!(commands.as_slice().iter().any(|command| matches!(
            command,
            Command::SetPageAsset {
                page: command_page,
                asset: command_asset,
                blob: None,
            } if *command_page == page && *command_asset == asset
        )));
    }
}

fn fake_pipeline(write_masks: bool) -> (Pipeline, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    fake_pipeline_with_config(write_masks, PipelineConfig::default())
}

fn ocr_pipeline(write_masks: bool) -> (Pipeline, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    fake_pipeline_with_config(
        write_masks,
        PipelineConfig {
            processors: vec![
                ProcessorConfig::ComicTextDetector(Default::default()),
                ProcessorConfig::MangaOcr(Default::default()),
            ],
        },
    )
}

fn fake_pipeline_with_config(
    write_masks: bool,
    config: PipelineConfig,
) -> (Pipeline, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let active_accelerator = Arc::new(AtomicUsize::new(0));
    let maximum_accelerator = Arc::new(AtomicUsize::new(0));
    let mut pipeline = Pipeline::with_factory(
        Config::memory(config),
        Config::memory(TranslationConfig::default()),
        Arc::new(FakeFactory {
            active,
            maximum: maximum.clone(),
            active_accelerator,
            maximum_accelerator: maximum_accelerator.clone(),
            write_masks,
        }),
    );
    pipeline.device = Device::cpu();
    (pipeline, maximum, maximum_accelerator)
}

fn session() -> Session {
    let mut session = Session::memory().unwrap();
    let mut commands = session.commands();
    let page = commands.add_page("page", source_png()).unwrap();
    commands
        .set_asset(page, PageAsset::BrushMask, Some(mask_png(8, 8).unwrap()))
        .unwrap();
    session.apply(commands).unwrap();
    session
}

fn source_png() -> Arc<[u8]> {
    encode(&DynamicImage::ImageRgb8(RgbImage::from_pixel(
        8,
        8,
        Rgb([255; 3]),
    )))
}

fn mask_png(width: u32, height: u32) -> Result<Arc<[u8]>> {
    Ok(encode(&DynamicImage::ImageLuma8(GrayImage::from_pixel(
        width,
        height,
        Luma([255]),
    ))))
}

fn encode(image: &DynamicImage) -> Arc<[u8]> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    Arc::from(bytes.into_inner())
}
