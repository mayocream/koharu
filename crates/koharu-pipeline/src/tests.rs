use super::*;

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

    assert!(Pipeline::new(PipelineConfig::default(), translation).is_err());
}

#[test]
fn reconfiguration_builds_a_new_immutable_pipeline() {
    let pipeline = Pipeline::new(PipelineConfig::default(), Default::default()).unwrap();
    let translation = koharu_translator::TranslationConfig {
        target_language: "ja-JP".to_owned(),
        ..Default::default()
    };
    let replacement = pipeline
        .reconfigured(PipelineConfig::default(), translation)
        .unwrap();

    assert_eq!(pipeline.configuration().1.target_language, "en-US");
    assert_eq!(replacement.configuration().1.target_language, "ja-JP");
}

#[tokio::test]
async fn stop_is_a_successful_partial_result() {
    let pipeline = Pipeline::new(PipelineConfig::default(), Default::default()).unwrap();
    let stop = StopToken::default();
    stop.stop();
    let request = Request {
        stop,
        ..Request::default()
    };
    let mut committer = RejectCommitter;

    let report = pipeline
        .execute(
            koharu_scene::SceneSession::memory().unwrap().snapshot(),
            request,
            &mut committer,
        )
        .await
        .unwrap();

    assert_eq!(report.status, RunStatus::Stopped);
    assert_eq!(report.completed, 0);
}

#[tokio::test]
async fn stop_after_a_page_keeps_completed_progress() {
    let translation = koharu_translator::TranslationConfig {
        model: koharu_translator::Providers::OpenAi(Default::default()),
        ..Default::default()
    };
    let pipeline = Pipeline::new(PipelineConfig::default(), translation).unwrap();
    let mut session = koharu_scene::SceneSession::memory().unwrap();
    let patch = session
        .snapshot()
        .patch(|edit| {
            edit.add_page(
                koharu_scene::PageDraft::new("one", 1.0, 1.0),
                koharu_scene::At::End,
            )?;
            edit.add_page(
                koharu_scene::PageDraft::new("two", 1.0, 1.0),
                koharu_scene::At::End,
            )?;
            Ok(())
        })
        .unwrap();
    session.commit(patch).unwrap();
    let stop = StopToken::default();
    let progress_stop = stop.clone();
    let request = Request {
        operation: Operation::Only(Stage::Translation),
        stop,
        progress: Some(std::sync::Arc::new(move |event| {
            if matches!(event, Progress::Skipped { .. }) {
                progress_stop.stop();
            }
        })),
        ..Request::default()
    };
    let mut committer = RejectCommitter;

    let report = pipeline
        .execute(session.snapshot(), request, &mut committer)
        .await
        .unwrap();

    assert_eq!(report.status, RunStatus::Stopped);
    assert_eq!(report.completed, 1);
    assert_eq!(report.total, 2);
}

struct RejectCommitter;

#[async_trait::async_trait]
impl Committer for RejectCommitter {
    async fn commit(
        &mut self,
        _output: StageOutput,
    ) -> anyhow::Result<koharu_scene::SceneSnapshot> {
        anyhow::bail!("stopped execution must not commit")
    }
}

#[test]
fn operations_expand_to_the_supported_workflows() {
    assert_eq!(
        Operation::Through(Stage::Translation).stages(),
        vec![Stage::Detection, Stage::Ocr, Stage::Translation]
    );
    assert_eq!(
        Operation::Through(Stage::Inpainting).stages(),
        vec![Stage::Detection, Stage::Inpainting]
    );
    assert_eq!(
        Operation::Only(Stage::Translation).stages(),
        vec![Stage::Translation]
    );
}
