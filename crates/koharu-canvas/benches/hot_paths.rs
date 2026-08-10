use std::{collections::HashMap, hint::black_box};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
pub use koharu_canvas::{
    Brush, Error, MaskOverlay, MaskTarget, PageId, PagePoint, PhysicalSize, PixelRect, PixelSize,
    RasterStrokeCommit, Result, StrokeMode,
};
use vello::{
    Scene,
    kurbo::{Affine, Rect},
    peniko::{Color as VelloColor, Fill},
};

// Compile the private edit engines into the benchmark target. This keeps the
// production API small while measuring the exact implementation used by Canvas.
#[allow(dead_code)]
#[path = "../src/mask.rs"]
mod mask;
#[allow(dead_code)]
#[path = "../src/raster.rs"]
mod raster;

fn retained_scene(layer_count: usize) -> Scene {
    let mut scene = Scene::new();
    for index in 0..layer_count {
        let x = (index % 16) as f64 * 64.0;
        let y = (index / 16) as f64 * 64.0;
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            VelloColor::from_rgba8(24, 80, 160, 255),
            None,
            &Rect::new(x, y, x + 48.0, y + 48.0),
        );
    }
    scene
}

fn frame_fixture() -> koharu_renderer::Frame {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("benchmark runtime")
        .block_on(async {
            let mut session = koharu_scene::Session::memory()
                .await
                .expect("benchmark scene session");
            let snapshot = session.snapshot();
            let mut page = None;
            let patch = snapshot
                .patch(|edit| {
                    page = Some(edit.add_page(
                        koharu_scene::PageDraft::new("canvas benchmark", 4096.0, 4096.0),
                        koharu_scene::At::End,
                    )?);
                    Ok(())
                })
                .expect("benchmark page patch");
            let commit = session.commit(patch).await.expect("benchmark page commit");
            koharu_renderer::Renderer::new()
                .expect("benchmark renderer")
                .render(&commit.snapshot, page.expect("benchmark page"))
                .await
                .expect("benchmark frame")
        })
}

fn compose_viewport(retained: &Scene, camera: Affine) -> Scene {
    let mut scene = Scene::new();
    scene.push_clip_layer(
        Fill::NonZero,
        Affine::IDENTITY,
        &Rect::new(0.0, 0.0, 1920.0, 1080.0),
    );
    scene.push_clip_layer(Fill::NonZero, camera, &Rect::new(0.0, 0.0, 4096.0, 4096.0));
    scene.append(retained, Some(camera));
    scene.pop_layer();
    scene.pop_layer();
    scene
}

fn canvas_hot_paths(c: &mut Criterion) {
    let frame = frame_fixture();
    c.bench_function("canvas/sync/frame_clone", |b| {
        b.iter(|| black_box(koharu_renderer::Frame::clone(black_box(&frame))))
    });

    let retained = retained_scene(256);
    c.bench_function("canvas/render/retained_frame_append_256_layers", |b| {
        b.iter(|| {
            let mut installed = Scene::new();
            installed.append(black_box(&retained), None);
            black_box(installed)
        })
    });

    let camera = Affine::new([1.75, 0.0, 0.0, 1.75, 120.0, -64.0]);
    c.bench_function("canvas/render/viewport_scene_composition", |b| {
        b.iter(|| black_box(compose_viewport(black_box(&retained), camera)))
    });

    let points = (1..=256)
        .map(|index| PagePoint::new(index as f64 * 3.0, 80.0 + (index % 11) as f64))
        .collect::<Vec<_>>();
    c.bench_function("canvas/edit/retained_raster_preview_256_points", |b| {
        b.iter_batched(
            || {
                raster::RasterStrokeEdit::new(RasterStrokeCommit {
                    page: koharu_scene::EntityId::new(),
                    layer: None,
                    mode: StrokeMode::Paint,
                    color: [25, 60, 120, 180],
                    diameter: 24.0,
                    points: vec![PagePoint::new(0.0, 80.0)],
                })
            },
            |mut edit| {
                for point in &points {
                    edit.push_point(*point);
                }
                black_box(edit)
            },
            BatchSize::SmallInput,
        )
    });

    c.bench_function("canvas/edit/sparse_mask_4k_diagonal", |b| {
        b.iter_batched(
            || mask::MaskState::empty(PhysicalSize::new(4096, 4096)),
            |mut state| {
                let mut before = HashMap::new();
                black_box(state.paint(
                    PagePoint::new(8.0, 8.0),
                    PagePoint::new(4088.0, 4088.0),
                    Brush {
                        diameter: 24.0,
                        color: [0; 4],
                        mode: StrokeMode::Paint,
                    },
                    &mut before,
                ));
                black_box(state)
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, canvas_hot_paths);
criterion_main!(benches);
