use std::{hint::black_box, sync::Arc};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use koharu_storage::{ComponentKey, ComponentRecord, RecordId, Session};

const RECORDS: usize = 2_048;

fn key(name: &str) -> ComponentKey {
    ComponentKey::named(format!("dev.koharu.bench.{name}"), "default").unwrap()
}

fn value(bytes: impl Into<Arc<[u8]>>) -> ComponentRecord {
    ComponentRecord::new(1, bytes, [], []).unwrap()
}

fn populated() -> (Session, RecordId) {
    let mut session = Session::memory().expect("create benchmark document");
    let mut edit = session.snapshot().edit();
    let mut selected = None;
    for index in 0..RECORDS {
        let record = edit.insert_record().expect("insert benchmark record");
        edit.set_component(
            record,
            key("payload"),
            value(Arc::<[u8]>::from(vec![index as u8; 512])),
        )
        .expect("set benchmark component");
        selected = Some(record);
    }
    session
        .commit(edit.finish().expect("finish benchmark edit"))
        .expect("commit benchmark document");
    (session, selected.expect("records are non-empty"))
}

fn storage_benchmarks(criterion: &mut Criterion) {
    let (session, selected) = populated();
    let snapshot = session.snapshot();
    criterion.bench_function("storage/snapshot_clone", |bencher| {
        bencher.iter(|| black_box(snapshot.clone()));
    });
    criterion.bench_function("storage/component_lookup", |bencher| {
        bencher.iter(|| {
            black_box(
                snapshot
                    .component(black_box(selected), &key("payload"))
                    .unwrap(),
            )
        });
    });
    criterion.bench_function("storage/component_patch", |bencher| {
        bencher.iter(|| {
            black_box(
                snapshot
                    .patch(|edit| {
                        edit.set_component(selected, key("payload"), value(&b"changed"[..]))
                    })
                    .unwrap(),
            )
        });
    });
    let patch = snapshot
        .patch(|edit| edit.set_component(selected, key("payload"), value(&b"changed"[..])))
        .unwrap();
    criterion.bench_function("storage/preview", |bencher| {
        bencher.iter(|| black_box(snapshot.preview(black_box([&patch])).unwrap()));
    });

    let mut commit = criterion.benchmark_group("storage/commit");
    commit.sample_size(10);
    commit.bench_function("independent_components", |bencher| {
        bencher.iter_batched(
            || {
                let (session, selected) = populated();
                let snapshot = session.snapshot();
                let patch = snapshot
                    .patch(|edit| {
                        edit.set_component(selected, key("left"), value(&b"left"[..]))?;
                        edit.set_component(selected, key("right"), value(&b"right"[..]))
                    })
                    .unwrap();
                (session, patch)
            },
            |(mut session, patch)| {
                black_box(session.commit(patch).unwrap());
            },
            BatchSize::SmallInput,
        );
    });
    commit.finish();
}

criterion_group!(benches, storage_benchmarks);
criterion_main!(benches);
