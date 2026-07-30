use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use koharu_scene::{At, Authored, EntityId, PageDraft, SceneSession, SourceText};

const ENTITIES: usize = 2_048;

fn source(text: impl Into<String>) -> SourceText {
    SourceText {
        text: Authored::user(text.into()),
        language: None,
    }
}

fn populated() -> (SceneSession, EntityId) {
    let mut session = SceneSession::memory().expect("create benchmark scene");
    let mut selected = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(PageDraft::new("page", 1200.0, 1800.0), At::End)?;
            for index in 0..ENTITIES {
                let entity = edit.add_entity(page, At::End)?;
                if index % 4 == 0 {
                    edit.set_source_text(entity, source(format!("text {index}")))?;
                }
                selected = Some(entity);
            }
            Ok(())
        })
        .expect("build benchmark scene");
    session.commit(patch).expect("commit benchmark scene");
    (session, selected.expect("scene has entities"))
}

fn scene_benchmarks(criterion: &mut Criterion) {
    let (session, selected) = populated();
    let snapshot = session.snapshot();

    criterion.bench_function("scene/snapshot_clone", |bencher| {
        bencher.iter(|| black_box(snapshot.clone()));
    });
    criterion.bench_function("scene/entities_with_source_text", |bencher| {
        bencher.iter(|| {
            black_box(
                snapshot
                    .entities_with::<SourceText>("default")
                    .expect("query source text")
                    .count(),
            )
        });
    });
    criterion.bench_function("scene/component_patch", |bencher| {
        bencher.iter(|| {
            black_box(
                snapshot
                    .patch(|edit| edit.set_source_text(selected, source("changed")))
                    .expect("build component patch"),
            )
        });
    });
    let patch = snapshot
        .patch(|edit| edit.set_source_text(selected, source("changed")))
        .expect("build preview patch");
    criterion.bench_function("scene/component_preview", |bencher| {
        bencher.iter(|| black_box(snapshot.preview(black_box([&patch])).unwrap()));
    });
}

criterion_group!(benches, scene_benchmarks);
criterion_main!(benches);
