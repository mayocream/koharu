//! LLM lifecycle — load/unload. Actual local-model loading is skipped (it
//! would require downloading multi-GB weights); we verify the routes wire up
//! and error sensibly on bad targets.

use koharu_client::apis::default_api as api;
use koharu_client::models;
use koharu_integration_tests::TestApp;

#[tokio::test]
async fn unload_when_empty_is_ok() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    api::delete_current_llm(&app.client_config).await?;
    let state = api::get_current_llm(&app.client_config).await?;
    assert_eq!(state.status, models::LlmStateStatus::Empty);
    Ok(())
}

#[tokio::test]
async fn load_with_bogus_local_model_errors() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let res = api::put_current_llm(
        &app.client_config,
        models::LlmLoadRequest {
            target: Box::new(models::LlmTarget {
                kind: models::LlmTargetKind::Local,
                model_id: "never-gonna-give-you-up".into(),
                provider_id: None,
            }),
            options: None,
        },
    )
    .await;
    assert!(res.is_err(), "unknown local model id should be rejected");
    Ok(())
}

#[tokio::test]
async fn load_with_provider_requiring_missing_secret_errors() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    // No provider secret configured — requires_api_key = true → Err.
    let res = api::put_current_llm(
        &app.client_config,
        models::LlmLoadRequest {
            target: Box::new(models::LlmTarget {
                kind: models::LlmTargetKind::Provider,
                model_id: "gpt-5-mini".into(),
                provider_id: Some(Some("openai".into())),
            }),
            options: None,
        },
    )
    .await;
    assert!(res.is_err(), "missing API key should surface as an error");
    Ok(())
}
