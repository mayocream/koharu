use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use koharu_scene::{At, Authored, EntityId, PageDraft, Session, SourceText};
use tokio::runtime::Runtime;

const ENTITIES: usize = 2_048;

fn source(text: impl Into<String>) -> SourceText {
    SourceText {
        text: Authored::user(text.into()),
        language: None,
    }
}

fn populated(runtime: &Runtime) -> (Session, EntityId) {
    let mut session = runtime
        .block_on(Session::memory())
        .expect("create benchmark scene");
    let mut selected = None;
    let patch = session
        .snapshot()
        .patch(|edit| {
            let page = edit.add_page(PageDraft::new("page", 1200.0, 1800.0), At::End)?;
            for index in 0..ENTITIES {
                if index % 4 == 0 {
                    let content = edit.add_text_content(page, At::End)?;
                    edit.set(content, &source(format!("text {index}")))?;
                    selected = Some(content);
                } else {
                    edit.add_entity(page, At::End)?;
                }
            }
            Ok(())
        })
        .expect("build benchmark scene");
    runtime
        .block_on(session.commit(patch))
        .expect("commit benchmark scene");
    (session, selected.expect("scene has entities"))
}

fn scene_benchmarks(criterion: &mut Criterion) {
    let runtime = Runtime::new().expect("create benchmark runtime");
    let (session, selected) = populated(&runtime);
    let snapshot = session.snapshot();

    criterion.bench_function("scene/snapshot_clone", |bencher| {
        bencher.iter(|| black_box(snapshot.clone()));
    });
    criterion.bench_function("scene/entities_with_source_text", |bencher| {
        bencher.iter(|| {
            black_box(
                snapshot
                    .entities_with::<SourceText>()
                    .expect("query source text")
                    .count(),
            )
        });
    });
    criterion.bench_function("scene/component_patch", |bencher| {
        bencher.iter(|| {
            black_box(
                snapshot
                    .patch(|edit| edit.set(selected, &source("changed")))
                    .expect("build component patch"),
            )
        });
    });
    let patch = snapshot
        .patch(|edit| edit.set(selected, &source("changed")))
        .expect("build preview patch");
    criterion.bench_function("scene/component_preview", |bencher| {
        bencher.iter(|| black_box(snapshot.preview(black_box([&patch])).unwrap()));
    });
    criterion.bench_function("scene/reorder_patch", |bencher| {
        bencher.iter(|| {
            black_box(
                snapshot
                    .patch(|edit| edit.move_entity(selected, snapshot.parent(selected)?, At::Start))
                    .expect("build reorder patch"),
            )
        });
    });
}

criterion_group!(benches, scene_benchmarks);
criterion_main!(benches);
