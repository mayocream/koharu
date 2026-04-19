//! Project lifecycle: create, open, close, list, import, export. All
//! projects live under the managed `{data.path}/projects/` directory; no
//! client-side paths involved.

use koharu_client::apis::default_api as api;
use koharu_client::models;
use koharu_integration_tests::TestApp;

#[tokio::test]
async fn create_and_close_project() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;

    let summary = api::create_project(
        &app.client_config,
        models::CreateProjectRequest {
            name: "Alpha Project".into(),
        },
    )
    .await?;
    assert_eq!(summary.name, "Alpha Project");
    assert_eq!(summary.id, "alpha-project");
    assert!(summary.path.contains("projects"));
    assert!(app.app.current_session().is_some());

    api::delete_current_project(&app.client_config).await?;
    assert!(app.app.current_session().is_none());
    Ok(())
}

#[tokio::test]
async fn reopen_project_by_id() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;

    api::create_project(
        &app.client_config,
        models::CreateProjectRequest {
            name: "Beta".into(),
        },
    )
    .await?;
    api::delete_current_project(&app.client_config).await?;

    let reopened = api::put_current_project(
        &app.client_config,
        models::OpenProjectRequest { id: "beta".into() },
    )
    .await?;
    assert_eq!(reopened.name, "Beta");
    assert_eq!(reopened.id, "beta");
    Ok(())
}

#[tokio::test]
async fn open_unknown_id_is_404() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    let err = api::put_current_project(
        &app.client_config,
        models::OpenProjectRequest {
            id: "does-not-exist".into(),
        },
    )
    .await;
    assert!(err.is_err(), "unknown id should error");
    Ok(())
}

#[tokio::test]
async fn create_collision_gets_suffixed_id() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;

    let first = api::create_project(
        &app.client_config,
        models::CreateProjectRequest {
            name: "Same Name".into(),
        },
    )
    .await?;
    api::delete_current_project(&app.client_config).await?;

    let second = api::create_project(
        &app.client_config,
        models::CreateProjectRequest {
            name: "Same Name".into(),
        },
    )
    .await?;
    assert_eq!(first.id, "same-name");
    assert_eq!(second.id, "same-name-1");
    Ok(())
}

#[tokio::test]
async fn list_projects_enumerates_managed_dir() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;

    api::create_project(
        &app.client_config,
        models::CreateProjectRequest {
            name: "First".into(),
        },
    )
    .await?;
    api::delete_current_project(&app.client_config).await?;
    api::create_project(
        &app.client_config,
        models::CreateProjectRequest {
            name: "Second".into(),
        },
    )
    .await?;
    api::delete_current_project(&app.client_config).await?;

    let listing = api::list_projects(&app.client_config).await?;
    let ids: Vec<_> = listing.projects.iter().map(|p| p.id.clone()).collect();
    assert!(ids.contains(&"first".to_string()), "ids = {ids:?}");
    assert!(ids.contains(&"second".to_string()), "ids = {ids:?}");
    Ok(())
}

#[tokio::test]
async fn export_empty_project_as_khr() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    api::create_project(
        &app.client_config,
        models::CreateProjectRequest {
            name: "Gamma".into(),
        },
    )
    .await?;

    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/projects/current/export", app.base_url))
        .json(&serde_json::json!({ "format": "khr" }))
        .send()
        .await?;
    assert!(res.status().is_success(), "status: {}", res.status());
    let content_type = res
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("application/octet-stream"),
        "content-type: {content_type}",
    );
    let body = res.bytes().await?;
    assert!(body.len() >= 4, "body too short");
    assert_eq!(&body[..2], b"PK", "not a zip");
    Ok(())
}

#[tokio::test]
async fn import_khr_round_trips() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    api::create_project(
        &app.client_config,
        models::CreateProjectRequest {
            name: "Delta".into(),
        },
    )
    .await?;

    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/projects/current/export", app.base_url))
        .json(&serde_json::json!({ "format": "khr" }))
        .send()
        .await?;
    assert!(res.status().is_success());
    let archive_bytes = res.bytes().await?;

    api::delete_current_project(&app.client_config).await?;

    let res = client
        .post(format!("{}/projects/import", app.base_url))
        .header("Content-Type", "application/zip")
        .body(archive_bytes)
        .send()
        .await?;
    assert!(res.status().is_success(), "status: {}", res.status());
    let summary: serde_json::Value = res.json().await?;
    assert_eq!(summary["name"], "Delta");
    let alloc_path = summary["path"].as_str().expect("path");
    assert!(
        alloc_path.contains("projects"),
        "server path should include projects/: {alloc_path}",
    );
    assert!(
        std::path::Path::new(alloc_path).exists(),
        "alloc dir exists"
    );
    Ok(())
}
