use std::{collections::BTreeMap, hint::black_box, sync::Arc};

use criterion::{Criterion, criterion_group, criterion_main};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use koharu_renderer::Renderer;
use koharu_scene::{
    AssetInput, AssetMetadata, AssetRef, AssetRole, At, Origin, PageDraft, PixelLayer, Session,
};
use vello::Scene;

fn fixture() -> (koharu_scene::Snapshot, koharu_scene::EntityId) {
    let mut encoded = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(1920, 1080, Rgba([255; 4])))
        .write_to(&mut encoded, ImageFormat::Png)
        .unwrap();
    let mut session = Session::memory().unwrap();
    let role = AssetRole::new("original").unwrap();
    let mut page_id = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(PageDraft::new("Page", 1920.0, 1080.0), At::End)?;
            edit.set_asset(
                page,
                &role,
                AssetInput::new(
                    Arc::<[u8]>::from(encoded.get_ref().clone()),
                    "image/png",
                    AssetMetadata {
                        width: Some(1920),
                        height: Some(1080),
                        attributes: BTreeMap::new(),
                    },
                ),
            )?;
            edit.set(
                page,
                &PixelLayer::color(Origin::User, "Original", AssetRef::new(page, role.clone())),
            )?;
            page_id = Some(page);
            Ok(())
        })
        .unwrap();
    let snapshot = session.commit(patch).unwrap().snapshot;
    (snapshot, page_id.unwrap())
}

fn rendering_benchmark(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let renderer = Renderer::new().unwrap();
    let (snapshot, page) = fixture();
    let composition = runtime.block_on(renderer.compose(&snapshot, page)).unwrap();

    c.bench_function("compose_retained", |b| {
        b.iter(|| {
            black_box(
                runtime
                    .block_on(renderer.compose(black_box(&snapshot), page))
                    .unwrap(),
            );
        })
    });
    c.bench_function("append_composition", |b| {
        b.iter(|| {
            let mut scene = Scene::new();
            composition.append_to(&mut scene, None);
            black_box(scene);
        })
    });
}

criterion_group!(benches, rendering_benchmark);
criterion_main!(benches);
