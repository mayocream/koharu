//! Meta, engines, config, fonts, LLM catalog — read-mostly endpoints.

use koharu_client::apis::default_api as api;
use koharu_client::models;
use koharu_integration_tests::TestApp;

#[tokio::test]
async fn meta_returns_version_and_device() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let meta = api::get_meta(&app.client_config).await?;
    assert_eq!(meta.version, "test");
    assert!(!meta.ml_device.is_empty());
    Ok(())
}

#[tokio::test]
async fn engine_catalog_lists_registered_engines() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let catalog = api::get_engine_catalog(&app.client_config).await?;
    // Every registered engine emits via inventory. The DEFAULT_PIPELINE
    // always expects these buckets to be non-empty.
    assert!(
        !catalog.detectors.is_empty(),
        "expected detectors registered"
    );
    assert!(!catalog.ocr.is_empty(), "expected OCR engines registered");
    assert!(
        !catalog.inpainters.is_empty(),
        "expected inpainters registered"
    );
    assert!(
        !catalog.renderers.is_empty(),
        "expected renderers registered"
    );
    Ok(())
}

#[tokio::test]
async fn config_get_returns_defaults() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let config = api::get_config(&app.client_config).await?;
    let http = config.http.expect("http section present");
    assert_eq!(http.connect_timeout, Some(20));
    assert_eq!(http.read_timeout, Some(300));
    Ok(())
}

#[tokio::test]
async fn config_patch_merges_and_persists() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;

    let patch = models::ConfigPatch {
        http: Some(Some(Box::new(models::HttpConfigPatch {
            connect_timeout: Some(Some(42)),
            read_timeout: None,
            max_retries: None,
        }))),
        pipeline: None,
        providers: None,
    };
    let updated = api::patch_config(&app.client_config, patch).await?;
    let http = updated.http.expect("http section");
    assert_eq!(http.connect_timeout, Some(42));
    assert_eq!(http.read_timeout, Some(300), "unchanged field stays");

    let fetched = api::get_config(&app.client_config).await?;
    assert_eq!(
        fetched.http.expect("http section").connect_timeout,
        Some(42)
    );
    Ok(())
}

#[tokio::test]
async fn fonts_endpoint_returns_available_fonts() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let _fonts = api::list_fonts(&app.client_config).await?;
    // Endpoint must respond successfully; list length is environment-dependent.
    Ok(())
}

#[tokio::test]
async fn google_fonts_catalog_is_non_empty() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let catalog = api::get_google_fonts_catalog(&app.client_config).await?;
    assert!(
        !catalog.fonts.is_empty(),
        "bundled catalog should have entries"
    );
    assert!(catalog.fonts.iter().all(|e| !e.family.is_empty()));
    Ok(())
}

#[tokio::test]
async fn llm_catalog_lists_local_models() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let catalog = api::get_catalog(&app.client_config).await?;
    assert!(
        !catalog.local_models.is_empty(),
        "at least one local LLM model should be registered"
    );
    Ok(())
}

#[tokio::test]
async fn llm_state_starts_empty() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let state = api::get_current_llm(&app.client_config).await?;
    assert_eq!(state.status, models::LlmStateStatus::Empty);
    Ok(())
}

#[tokio::test]
async fn provider_secret_set_and_clear() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    api::set_provider_secret(
        &app.client_config,
        "openai",
        models::ProviderSecretRequest {
            secret: "sk-test".into(),
        },
    )
    .await?;
    // Config now lists provider; api_key is redacted in the response.
    let cfg = api::get_config(&app.client_config).await?;
    let providers = cfg.providers.expect("providers list");
    let openai = providers
        .iter()
        .find(|p| p.id == "openai")
        .expect("provider should be registered");
    assert!(openai.api_key.clone().flatten().is_some());

    api::clear_provider_secret(&app.client_config, "openai").await?;
    let cleared = api::get_config(&app.client_config).await?;
    let cleared_providers = cleared.providers.expect("providers list");
    let cleared_openai = cleared_providers
        .iter()
        .find(|p| p.id == "openai")
        .expect("provider entry remains after clear");
    assert!(cleared_openai.api_key.clone().flatten().is_none());
    Ok(())
}
