//! Binary reads: /scene.bin, /blobs/:hash, /pages/:id/thumbnail.

use koharu_integration_tests::TestApp;
use reqwest::multipart::{Form, Part};

async fn import_one(app: &TestApp, png: Vec<u8>) -> anyhow::Result<String> {
    let form = Form::new().part(
        "file",
        Part::bytes(png)
            .file_name("p.png".to_string())
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
    let v: serde_json::Value = resp.json().await?;
    Ok(v["pages"][0].as_str().unwrap().to_string())
}

#[tokio::test]
async fn scene_bin_deserializes_with_postcard() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("sb").await?;
    import_one(&app, TestApp::tiny_png(12, 12, [1, 2, 3, 255])).await?;

    let resp = app
        .client_config
        .client
        .get(format!("{}/scene.bin", app.base_url))
        .send()
        .await?
        .error_for_status()?;
    let epoch_header = resp
        .headers()
        .get("x-koharu-epoch")
        .expect("epoch header set");
    assert!(epoch_header.to_str()?.parse::<u64>()? > 0);

    let bytes = resp.bytes().await?;
    // Wire format is `Snapshot { epoch, scene }`. We only peek `scene` via
    // a tuple-compatible struct definition — avoids depending on the wire
    // struct being re-exported.
    #[derive(serde::Deserialize)]
    struct WireSnapshot {
        epoch: u64,
        scene: koharu_core::Scene,
    }
    let snap: WireSnapshot = postcard::from_bytes(&bytes)?;
    assert!(snap.epoch > 0);
    assert_eq!(snap.scene.pages.len(), 1);
    Ok(())
}

#[tokio::test]
async fn blob_fetch_returns_stored_bytes() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("bl").await?;
    let original = TestApp::tiny_png(16, 16, [99, 99, 99, 255]);
    import_one(&app, original.clone()).await?;

    // Pull the source blob ref out of the scene.
    let blob = {
        let session = app.app.current_session().unwrap();
        let scene = session.scene.read();
        let page = scene.pages.values().next().unwrap();
        page.nodes
            .values()
            .find_map(|n| match &n.kind {
                koharu_core::NodeKind::Image(i) if i.role == koharu_core::ImageRole::Source => {
                    Some(i.blob.clone())
                }
                _ => None,
            })
            .expect("source blob ref")
    };

    let resp = app
        .client_config
        .client
        .get(format!("{}/blobs/{}", app.base_url, blob.hash()))
        .send()
        .await?
        .error_for_status()?;
    let body = resp.bytes().await?;
    assert_eq!(body.as_ref(), original.as_slice());
    Ok(())
}

#[tokio::test]
async fn thumbnail_returns_webp() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("th").await?;
    let png = TestApp::tiny_png(500, 500, [7, 8, 9, 255]);
    let page_id = import_one(&app, png).await?;

    let resp = app
        .client_config
        .client
        .get(format!("{}/pages/{}/thumbnail", app.base_url, page_id))
        .send()
        .await?
        .error_for_status()?;
    let content_type = resp.headers().get("content-type").cloned();
    let bytes = resp.bytes().await?;
    assert_eq!(
        content_type.as_ref().and_then(|v| v.to_str().ok()),
        Some("image/webp")
    );
    // Decode to confirm it's a real WebP + downscaled.
    let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)?;
    assert!(decoded.width() <= 320 && decoded.height() <= 320);
    Ok(())
}

#[tokio::test]
async fn thumbnail_is_cached_on_disk() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("thc").await?;
    let page_id = import_one(&app, TestApp::tiny_png(64, 64, [1, 1, 1, 255])).await?;

    // First call generates and caches.
    app.client_config
        .client
        .get(format!("{}/pages/{}/thumbnail", app.base_url, page_id))
        .send()
        .await?
        .error_for_status()?;

    // Second call should serve the cached file — check it exists.
    let session = app.app.current_session().unwrap();
    let cache_path = session
        .dir
        .join("cache")
        .join("thumbs")
        .join(format!("{page_id}.webp"));
    assert!(cache_path.exists());
    Ok(())
}

#[tokio::test]
async fn blob_missing_returns_404() -> anyhow::Result<()> {
    let app = TestApp::spawn().await?;
    app.open_fresh_project("404").await?;

    let resp = app
        .client_config
        .client
        .get(format!("{}/blobs/{}", app.base_url, "deadbeefdeadbeef"))
        .send()
        .await?;
    assert_eq!(resp.status(), 404);
    Ok(())
}
