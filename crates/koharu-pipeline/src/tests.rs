use super::*;

use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use async_trait::async_trait;

#[test]
fn construction_does_not_load_models() {
    let pipeline = Pipeline::new(PipelineConfig::default(), Default::default()).unwrap();
    assert!(
        pipeline
            .model_status()
            .iter()
            .filter(|status| status.active_configuration)
            .all(|status| matches!(status.load, LoadState::Unloaded | LoadState::NotRequired))
    );
}

#[test]
fn reconfiguration_advances_atomically() {
    let pipeline = Pipeline::new(PipelineConfig::default(), Default::default()).unwrap();
    pipeline
        .model_status
        .load(ConfigRevision(1), Stage::Detection, LoadState::Loaded);
    let translation = koharu_translator::TranslationConfig {
        target_language: "ja-JP".to_owned(),
        ..Default::default()
    };
    let change = pipeline
        .reconfigure(PipelineConfig::default(), translation)
        .unwrap();
    assert_eq!(change.revision, ConfigRevision(2));
    assert!(change.changed.is_empty());
    assert_eq!(pipeline.configuration().0, ConfigRevision(2));
    let statuses = pipeline.model_status();
    assert_eq!(statuses.len(), Stage::ALL.len());
    assert!(statuses.iter().any(|status| {
        status.generation == ConfigRevision(2)
            && status.stage == Stage::Detection
            && status.load == LoadState::Loaded
    }));
}

#[test]
fn concurrent_reconfiguration_never_regresses_the_active_revision() {
    let pipeline = Arc::new(Pipeline::new(PipelineConfig::default(), Default::default()).unwrap());
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads = (0..8)
        .map(|index| {
            let pipeline = pipeline.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let translation = koharu_translator::TranslationConfig {
                    instructions: Some(format!("revision-{index}")),
                    ..Default::default()
                };
                pipeline
                    .reconfigure(PipelineConfig::default(), translation)
                    .unwrap()
                    .revision
            })
        })
        .collect::<Vec<_>>();
    let revisions = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(pipeline.configuration().0, *revisions.iter().max().unwrap());
    assert_eq!(pipeline.configuration().0, ConfigRevision(9));
}

#[test]
fn configuration_rejects_unknown_fields() {
    let result = toml::from_str::<PipelineConfig>("legacy_limit = 1");
    assert!(result.is_err());
}

#[test]
fn construction_rejects_an_unknown_local_translation_model() {
    let translation = koharu_translator::TranslationConfig {
        model: koharu_translator::Providers::Local(koharu_translator::LocalConfig {
            model: "missing-model".to_owned(),
        }),
        ..Default::default()
    };

    let result = Pipeline::new(PipelineConfig::default(), translation);

    assert!(result.is_err());
}

#[test]
fn graph_uses_stable_stage_identity() {
    let pipeline = Pipeline::new(PipelineConfig::default(), Default::default()).unwrap();
    let graph = pipeline.graph();
    assert!(graph.contains("detection -> ocr"));
    assert!(graph.contains("ocr -> translation"));
    assert!(!graph.contains("n0"));
}

struct ConcurrentProcessor {
    spec: ProcessorSpec,
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
}

#[async_trait]
impl Processor for ConcurrentProcessor {
    fn spec(&self) -> &ProcessorSpec {
        &self.spec
    }

    async fn ensure_loaded(&self, _: &LoadContext) -> Result<()> {
        Ok(())
    }

    async fn run(&self, input: NodeInput) -> Result<NodeOutput> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(NodeOutput {
            patch: input.scene.patch(|_| Ok(()))?,
            artifacts: Default::default(),
            measurements: Default::default(),
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_ready_stages_run_concurrently() {
    let pipeline = Pipeline::new(PipelineConfig::default(), Default::default()).unwrap();
    let maximum = install_concurrent_processors(&pipeline);

    let snapshot = koharu_scene::SceneSession::memory().unwrap().snapshot();
    let report = pipeline.run(snapshot).execute().await.unwrap();

    assert_eq!(report.nodes.len(), Stage::ALL.len());
    assert!(maximum.load(Ordering::SeqCst) >= 2);
}

fn install_concurrent_processors(pipeline: &Pipeline) -> Arc<AtomicUsize> {
    let previous = pipeline.current.load_full();
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let processors = Stage::ALL
        .into_iter()
        .map(|stage| {
            let processor: Arc<dyn Processor> = Arc::new(ConcurrentProcessor {
                spec: ProcessorSpec {
                    stage,
                    model: format!("fake-{stage}"),
                    local: false,
                },
                active: active.clone(),
                maximum: maximum.clone(),
            });
            (stage, processor)
        })
        .collect();
    pipeline.current.store(Arc::new(ConfigurationGeneration {
        revision: previous.revision,
        pipeline: previous.pipeline.clone(),
        translation: previous.translation.clone(),
        nodes: previous.nodes.clone(),
        processors,
        usage: Stage::ALL
            .into_iter()
            .map(|stage| (stage, Arc::new(tokio::sync::Mutex::new(()))))
            .collect(),
    }));
    maximum
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_has_one_typed_terminal_result() {
    let pipeline = Pipeline::new(PipelineConfig::default(), Default::default()).unwrap();
    install_concurrent_processors(&pipeline);
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let snapshot = koharu_scene::SceneSession::memory().unwrap().snapshot();

    let error = pipeline
        .run(snapshot)
        .cancellation(cancellation)
        .execute()
        .await
        .unwrap_err();

    assert!(error.is_cancelled());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preflight_only_validates_selected_pages() {
    let pipeline = Pipeline::new(PipelineConfig::default(), Default::default()).unwrap();
    install_concurrent_processors(&pipeline);
    let mut session = koharu_scene::SceneSession::memory().unwrap();
    let mut selected = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(
                koharu_scene::PageDraft::new("selected", 1.0, 1.0),
                koharu_scene::At::End,
            )?;
            edit.set_asset(
                page,
                &koharu_scene::AssetRole::new("source")?,
                koharu_scene::AssetInput::new(
                    Arc::<[u8]>::from([0_u8]),
                    "image/png",
                    koharu_scene::AssetMetadata {
                        width: Some(1),
                        height: Some(1),
                        attributes: Default::default(),
                    },
                ),
            )?;
            edit.add_page(
                koharu_scene::PageDraft::new("unselected", 1.0, 1.0),
                koharu_scene::At::End,
            )?;
            selected = Some(page);
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;

    let report = pipeline
        .run(snapshot)
        .pages([selected.unwrap()])
        .execute()
        .await
        .unwrap();

    assert_eq!(report.nodes.len(), Stage::ALL.len());
}
