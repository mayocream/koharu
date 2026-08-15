use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use koharu_rasterizer::{PREPARED_RASTER_TILE_DIMENSION, RasterOptions, Rasterizer};
use koharu_renderer::{ImageKind, LayerKind, Renderer};
use koharu_scene::{
    AssetInput, AssetMetadata, AssetRole, At, Origin, PageDraft, RasterLayer, RasterLayerKind,
    Session,
};

fn png(width: u32, height: u32, color: [u8; 4]) -> Arc<[u8]> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba(color));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner().into()
}

fn png_image(image: image::RgbaImage) -> Arc<[u8]> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner().into()
}

#[tokio::test]
async fn public_renderer_returns_a_complete_immutable_frame() {
    let mut session = Session::memory().await.unwrap();
    let mut page = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            page = Some(edit.add_page(PageDraft::new("page", 320.0, 240.0), At::End)?);
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).await.unwrap().snapshot;
    let page = page.unwrap();

    let renderer = Renderer::new().unwrap();
    let frame = renderer.render(&snapshot, page).await.unwrap();

    assert_eq!(frame.page(), page);
    assert_eq!(frame.revision(), snapshot.revision());
    assert_eq!(frame.size(), (320, 240));
    assert_eq!(frame.origin(), (0, 0));
    assert!(frame.layers().is_empty());
    assert!(frame.layer(page).is_none());
    assert!(frame
        .diagnostics()
        .iter()
        .any(|diagnostic| matches!(diagnostic, koharu_renderer::RenderDiagnostic::MissingAsset { entity, role } if *entity == page && role == "source")));
}

#[tokio::test]
async fn native_rasterization_preserves_pixels_across_prepared_tile_edges() {
    let width = PREPARED_RASTER_TILE_DIMENSION + 1;
    let red = [230, 20, 30, 255];
    let blue = [10, 40, 220, 255];
    let mut source_image = image::RgbaImage::from_pixel(width, 1, image::Rgba(red));
    source_image.put_pixel(width - 1, 0, image::Rgba(blue));
    let source = AssetRole::new("source").unwrap();
    let mut session = Session::memory().await.unwrap();
    let mut page = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let created = edit.add_page(PageDraft::new("page", width as f64, 1.0), At::End)?;
            edit.set_asset(
                created,
                &source,
                AssetInput::new(
                    png_image(source_image),
                    "image/png",
                    AssetMetadata {
                        width: Some(width),
                        height: Some(1),
                        attributes: BTreeMap::new(),
                    },
                ),
            )?;
            page = Some(created);
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).await.unwrap().snapshot;
    let frame = Renderer::new()
        .unwrap()
        .render(&snapshot, page.unwrap())
        .await
        .unwrap();
    let manifest = frame.prepared().manifest().unwrap();
    assert_eq!(manifest.resources.len(), 1);
    let koharu_rasterizer::PreparedContent::Raster(prepared) = &manifest.frame.layers[0].content
    else {
        panic!("source layer must remain raster");
    };
    assert_eq!(prepared.tiles.len(), 2);

    let raster = Rasterizer::new()
        .unwrap()
        .rasterize(&frame.raster_frame().unwrap(), RasterOptions::default())
        .unwrap();

    assert_eq!(raster.image.get_pixel(width - 2, 0).0, red);
    assert_eq!(raster.image.get_pixel(width - 1, 0).0, blue);
}

#[tokio::test]
async fn discarded_image_nodes_are_rebuilt_for_a_reopened_presentation() {
    let mut session = Session::memory().await.unwrap();
    let source = AssetRole::new("source").unwrap();
    let color = [12, 34, 56, 255];
    let mut page = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let created = edit.add_page(PageDraft::new("page", 4.0, 4.0), At::End)?;
            edit.set_asset(
                created,
                &source,
                AssetInput::new(
                    png(4, 4, color),
                    "image/png",
                    AssetMetadata {
                        width: Some(4),
                        height: Some(4),
                        attributes: BTreeMap::new(),
                    },
                ),
            )?;
            page = Some(created);
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).await.unwrap().snapshot;
    let page = page.unwrap();
    let renderer = Renderer::new().unwrap();

    renderer.render(&snapshot, page).await.unwrap();
    renderer.discard_retained_nodes();
    let reopened = renderer.render(&snapshot, page).await.unwrap();
    let raster = Rasterizer::new()
        .unwrap()
        .rasterize(&reopened.raster_frame().unwrap(), RasterOptions::default())
        .unwrap();

    assert_eq!(reopened.stats().rebuilt_layers, 1);
    assert!(raster.image.pixels().all(|pixel| pixel.0 == color));
}

#[tokio::test]
async fn rasterized_png_contains_source_and_cleanup_pixels() {
    let mut session = Session::memory().await.unwrap();
    let source = AssetRole::new("source").unwrap();
    let mut cleanup = image::RgbaImage::new(4, 4);
    cleanup.put_pixel(2, 1, image::Rgba([240, 30, 20, 255]));
    let mut page = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let created = edit.add_page(PageDraft::new("page", 4.0, 4.0), At::End)?;
            edit.set_asset(
                created,
                &source,
                AssetInput::new(
                    png(4, 4, [10, 20, 200, 255]),
                    "image/png",
                    AssetMetadata {
                        width: Some(4),
                        height: Some(4),
                        attributes: BTreeMap::new(),
                    },
                ),
            )?;
            let cleanup_layer = edit.add_entity(created, At::Start)?;
            edit.set(
                cleanup_layer,
                &RasterLayer {
                    origin: Origin::User,
                    name: "Cleanup".to_owned(),
                    kind: RasterLayerKind::Cleanup,
                },
            )?;
            edit.set_asset(
                cleanup_layer,
                &source,
                AssetInput::new(
                    png_image(cleanup),
                    "image/png",
                    AssetMetadata {
                        width: Some(4),
                        height: Some(4),
                        attributes: BTreeMap::new(),
                    },
                ),
            )?;
            page = Some(created);
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).await.unwrap().snapshot;
    let renderer = Renderer::new().unwrap();
    let frame = renderer.render(&snapshot, page.unwrap()).await.unwrap();
    let raster = Rasterizer::new()
        .unwrap()
        .rasterize(&frame.raster_frame().unwrap(), RasterOptions::default())
        .unwrap();

    assert!(frame.layers().iter().any(|layer| matches!(
        layer.kind(),
        LayerKind::Image(metadata) if metadata.kind == ImageKind::Source
    )));
    assert!(frame.layers().iter().any(|layer| matches!(
        layer.kind(),
        LayerKind::Image(metadata) if metadata.kind == ImageKind::Cleanup
    )));
    assert_eq!(raster.image.get_pixel(0, 0).0, [10, 20, 200, 255]);
    assert_eq!(raster.image.get_pixel(2, 1).0, [240, 30, 20, 255]);
}

#[test]
fn public_layer_kinds_remain_export_metadata() {
    let _ = LayerKind::Image(koharu_renderer::ImageMetadata {
        name: None,
        kind: ImageKind::Source,
    });
}
