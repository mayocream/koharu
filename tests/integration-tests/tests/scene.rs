//! Scene mutation: multipart page import, /history/apply + undo/redo, batch.
//!
//! The generated client doesn't wire multipart bodies or typed `Op` unions
//! well, so scene mutations go through reqwest directly with `koharu_core::Op`
//! serialized as JSON — end-to-end round trip through axum's JSON extractor
//! exercises exactly the wire format the frontend will use.

use koharu_core::{ImageRole, NodeKind, Op, PagePatch};
use koharu_integration_tests::TestApp;
use reqwest::multipart::{Form, Part};
use serde_json::Value;

async fn apply(app: &TestApp, op: Op) -> anyhow::Result<u64> {
    let resp = app
        .client_config
        .client
        .post(format!("{}/history/apply", app.base_url))
        .json(&op)
        .send()
        .await?
        .error_for_status()?;
    let v: Value = resp.json().await?;
    Ok(v["epoch"].as_u64().expect("epoch in response"))
}

async fn undo(app: &TestApp) -> anyhow::Result<Option<u64>> {
    let resp = app
        .client_config
        .client
        .post(format!("{}/history/undo", app.base_url))
        .send()
        .await?
        .error_for_status()?;
    let v: Value = resp.json().await?;
    Ok(v["epoch"].as_u64())
}

async fn redo(app: &TestApp) -> anyhow::Result<Option<u64>> {
    let resp = app
        .client_config
        .client
        .post(format!("{}/history/redo", app.base_url))
        .send()
        .await?
        .error_for_status()?;
    let v: Value = resp.json().await?;
    Ok(v["epoch"].as_u64())
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
    let v: Value = resp.json().await?;
    Ok(v["pages"]
        .as_array()
        .expect("pages array")
        .iter()
        .map(|id| id.as_str().expect("uuid string").to_string())
        .collect())
}

#[tokio::test]
async fn import_pages_creates_source_nodes() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("p").await?;

    let png = TestApp::tiny_png(32, 16, [255, 0, 0, 255]);
    let ids = import_pages(&app, vec![("a.png", png.clone()), ("b.png", png.clone())]).await?;
    assert_eq!(ids.len(), 2);

    let session = app.app.current_session().expect("session");
    let scene = session.scene.read();
    assert_eq!(scene.pages.len(), 2);
    for page in scene.pages.values() {
        assert_eq!(page.width, 32);
        assert_eq!(page.height, 16);
        // Each page has exactly one Source image node.
        let sources = page
            .nodes
            .values()
            .filter(|n| matches!(&n.kind, NodeKind::Image(i) if i.role == ImageRole::Source))
            .count();
        assert_eq!(sources, 1);
    }
    Ok(())
}

#[tokio::test]
async fn update_page_then_undo_round_trips() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("r").await?;

    let png = TestApp::tiny_png(10, 10, [0, 128, 0, 255]);
    let ids = import_pages(&app, vec![("pg.png", png)]).await?;
    let page_id: koharu_core::PageId = ids[0].parse::<uuid::Uuid>().map(koharu_core::PageId)?;

    let epoch_before = app.app.current_session().unwrap().epoch();

    let op = Op::UpdatePage {
        id: page_id,
        patch: PagePatch {
            name: Some("renamed".into()),
            width: None,
            height: None,
        },
        prev: Default::default(),
    };
    let e1 = apply(&app, op).await?;
    assert!(e1 > epoch_before);
    {
        let session = app.app.current_session().unwrap();
        let scene = session.scene.read();
        assert_eq!(scene.page(page_id).unwrap().name, "renamed");
    }

    let e2 = undo(&app).await?.expect("undo produced epoch");
    assert!(e2 > e1);
    {
        let session = app.app.current_session().unwrap();
        let scene = session.scene.read();
        assert_eq!(scene.page(page_id).unwrap().name, "pg.png");
    }

    let e3 = redo(&app).await?.expect("redo produced epoch");
    assert!(e3 > e2);
    {
        let session = app.app.current_session().unwrap();
        let scene = session.scene.read();
        assert_eq!(scene.page(page_id).unwrap().name, "renamed");
    }
    Ok(())
}

#[tokio::test]
async fn batch_op_is_one_undo_step() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("b").await?;

    let png = TestApp::tiny_png(8, 8, [0, 0, 255, 255]);
    let ids = import_pages(&app, vec![("x.png", png.clone()), ("y.png", png)]).await?;
    let p1 = ids[0].parse::<uuid::Uuid>().map(koharu_core::PageId)?;
    let p2 = ids[1].parse::<uuid::Uuid>().map(koharu_core::PageId)?;

    let batch = Op::Batch {
        ops: vec![
            Op::UpdatePage {
                id: p1,
                patch: PagePatch {
                    name: Some("A".into()),
                    ..Default::default()
                },
                prev: Default::default(),
            },
            Op::UpdatePage {
                id: p2,
                patch: PagePatch {
                    name: Some("B".into()),
                    ..Default::default()
                },
                prev: Default::default(),
            },
        ],
        label: "rename both".into(),
    };
    apply(&app, batch).await?;
    {
        let session = app.app.current_session().unwrap();
        let scene = session.scene.read();
        assert_eq!(scene.page(p1).unwrap().name, "A");
        assert_eq!(scene.page(p2).unwrap().name, "B");
    }

    // One undo rolls back both renames.
    undo(&app).await?;
    {
        let session = app.app.current_session().unwrap();
        let scene = session.scene.read();
        assert_eq!(scene.page(p1).unwrap().name, "x.png");
        assert_eq!(scene.page(p2).unwrap().name, "y.png");
    }
    Ok(())
}

#[tokio::test]
async fn replace_flag_clears_prior_pages() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("rep").await?;

    let png = TestApp::tiny_png(4, 4, [1, 2, 3, 255]);
    import_pages(&app, vec![("old.png", png.clone())]).await?;

    // Replace with fresh import.
    let mut form = Form::new();
    form = form.text("replace", "true");
    form = form.part(
        "file",
        Part::bytes(TestApp::tiny_png(6, 6, [4, 5, 6, 255]))
            .file_name("new.png".to_string())
            .mime_str("image/png")?,
    );
    let resp = app
        .client_config
        .client
        .post(format!("{}/pages", app.base_url))
        .multipart(form)
        .send()
        .await?
        .error_for_status()?;
    let body: serde_json::Value = resp.json().await?;
    assert_eq!(body["pages"].as_array().unwrap().len(), 1);

    let session = app.app.current_session().unwrap();
    let scene = session.scene.read();
    assert_eq!(scene.pages.len(), 1);
    let page = scene.pages.values().next().unwrap();
    assert_eq!(page.name, "new.png");
    assert_eq!(page.width, 6);
    Ok(())
}

#[tokio::test]
async fn image_layer_adds_custom_node() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("il").await?;

    let png = TestApp::tiny_png(100, 100, [10, 20, 30, 255]);
    let ids = import_pages(&app, vec![("base.png", png)]).await?;
    let page_id = &ids[0];

    let form = Form::new().part(
        "file",
        Part::bytes(TestApp::tiny_png(20, 20, [200, 0, 0, 255]))
            .file_name("logo.png".to_string())
            .mime_str("image/png")?,
    );
    let resp = app
        .client_config
        .client
        .post(format!("{}/pages/{}/image-layers", app.base_url, page_id))
        .multipart(form)
        .send()
        .await?
        .error_for_status()?;
    let body: serde_json::Value = resp.json().await?;
    assert!(body["node"].is_string());

    let session = app.app.current_session().unwrap();
    let scene = session.scene.read();
    let page_uuid = page_id.parse::<uuid::Uuid>().map(koharu_core::PageId)?;
    let page = scene.page(page_uuid).unwrap();
    // Source + Custom.
    assert_eq!(page.nodes.len(), 2);
    let custom = page
        .nodes
        .values()
        .find(|n| matches!(&n.kind, NodeKind::Image(i) if i.role == ImageRole::Custom))
        .expect("custom node");
    let NodeKind::Image(img) = &custom.kind else {
        unreachable!()
    };
    assert_eq!(img.natural_width, 20);
    assert_eq!(img.natural_height, 20);
    Ok(())
}
