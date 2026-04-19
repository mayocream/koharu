//! Pipelines + operations + downloads. We don't run real engines (would require
//! loading multi-gigabyte models); we verify the routes surface.

use koharu_client::apis::default_api as api;
use koharu_client::models;
use koharu_integration_tests::TestApp;

fn empty_pipeline_request(steps: Vec<String>) -> models::StartPipelineRequest {
    models::StartPipelineRequest {
        steps,
        pages: None,
        region: None,
        target_language: None,
        system_prompt: None,
        default_font: None,
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
