//! Pipelines + operations + downloads. We don't run real engines (would require
//! loading multi-gigabyte models); we verify the routes surface.

use koharu_client::apis::default_api as api;
use koharu_client::models;
use koharu_core::{ImageRole, JobStatus, NodeKind, PageId};
use koharu_integration_tests::TestApp;
use reqwest::multipart::{Form, Part};
use tokio::time::{Duration, Instant, sleep};

fn empty_pipeline_request(steps: Vec<String>) -> models::StartPipelineRequest {
    models::StartPipelineRequest {
        batch_translation_char_limit: None,
        steps,
        pages: None,
        region: None,
        target_language: None,
        system_prompt: None,
        default_font: None,
    }
}

async fn import_pages(app: &TestApp, files: Vec<(&str, Vec<u8>)>) -> anyhow::Result<Vec<String>> {
    let mut form = Form::new();
    for (name, bytes) in files {
        form = form.part(
            "file",
            Part::bytes(bytes)
                .file_name(name.to_string())
                .mime_str("image/png")?,
        );
    }
    let resp = app
        .client_config
        .client
        .post(format!("{}/pages", app.base_url))
        .multipart(form)
        .send()
        .await?
        .error_for_status()?;
    let body: serde_json::Value = resp.json().await?;
    Ok(body["pages"]
        .as_array()
        .expect("pages array")
        .iter()
        .map(|id| id.as_str().expect("uuid string").to_string())
        .collect())
}

async fn wait_for_job(
    app: &TestApp,
    operation_id: &str,
) -> anyhow::Result<koharu_core::JobSummary> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(job) = app.app.jobs.get(operation_id) {
            let snapshot = job.value().clone();
            if snapshot.status != JobStatus::Running {
                return Ok(snapshot);
            }
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "timed out waiting for job {operation_id}"
        );
        sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn pipeline_without_project_errors() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let err = api::start_pipeline(
        &app.client_config,
        empty_pipeline_request(vec!["koharu-renderer".into()]),
    )
    .await;
    assert!(err.is_err(), "should 400 with no project open");
    Ok(())
}

#[tokio::test]
async fn pipeline_with_unknown_step_fails_fast() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("pp").await?;

    let resp = api::start_pipeline(
        &app.client_config,
        empty_pipeline_request(vec!["no-such-engine".into()]),
    )
    .await;
    assert!(resp.is_err());
    Ok(())
}

#[tokio::test]
async fn cancel_operation_accepts_unknown_id() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    // Best-effort: cancelling an unknown operation is a no-op 204.
    api::cancel_operation(&app.client_config, "nonexistent").await?;
    Ok(())
}

#[tokio::test]
async fn start_download_with_unknown_id_is_404() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let err = api::start_download(
        &app.client_config,
        models::StartDownloadRequest {
            model_id: "not-a-real-package".into(),
        },
    )
    .await;
    assert!(err.is_err(), "unknown package id should 404");
    Ok(())
}

#[tokio::test]
async fn cancel_operation_via_download_id_is_204() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    // Downloads share the unified cancel: DELETE /operations/{id}.
    api::cancel_operation(&app.client_config, "anything").await?;
    Ok(())
}

#[tokio::test]
async fn renderer_pipeline_noops_on_pages_without_text_blocks() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("render-noop").await?;

    let png = TestApp::tiny_png(24, 24, [255, 255, 255, 255]);
    let page_ids = import_pages(&app, vec![("a.png", png.clone()), ("b.png", png)]).await?;
    let pages = page_ids
        .iter()
        .map(|id| id.parse::<uuid::Uuid>())
        .collect::<Result<Vec<_>, _>>()?;

    let resp = api::start_pipeline(
        &app.client_config,
        models::StartPipelineRequest {
            batch_translation_char_limit: None,
            steps: vec!["koharu-renderer".into()],
            pages: Some(Some(pages)),
            region: None,
            target_language: None,
            system_prompt: None,
            default_font: None,
        },
    )
    .await?;

    let job = wait_for_job(&app, &resp.operation_id).await?;
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.error, None);

    let session = app.app.current_session().expect("session");
    let scene = session.scene.read();
    for page_id in page_ids {
        let page_id = page_id.parse::<uuid::Uuid>().map(PageId)?;
        let page = scene.page(page_id).expect("page exists");
        let rendered = page
            .nodes
            .values()
            .filter(|node| matches!(&node.kind, NodeKind::Image(img) if img.role == ImageRole::Rendered))
            .count();
        assert_eq!(rendered, 0, "renderer should no-op on textless page");
    }

    Ok(())
}
