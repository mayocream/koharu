use std::hint::black_box;

use bytes::Bytes;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use koharu_storage::{BlobId, DocumentId, Session};
use tokio::runtime::Runtime;

fn storage_benchmarks(criterion: &mut Criterion) {
    let runtime = Runtime::new().expect("create benchmark runtime");

    criterion.bench_function("storage/save_complete_state_512k", |bencher| {
        bencher.iter_batched(
            || {
                let document = DocumentId::new();
                let session = runtime
                    .block_on(Session::memory(document, Bytes::new()))
                    .expect("create benchmark project");
                let current = runtime.block_on(session.load()).expect("load state");
                let next = current
                    .update(
                        current.revision().next().unwrap(),
                        Bytes::from(vec![7; 512 * 1024]),
                        [],
                        [],
                    )
                    .expect("derive state");
                (session, next)
            },
            |(session, next)| black_box(runtime.block_on(session.save(&next)).expect("save state")),
            BatchSize::SmallInput,
        );
    });

    let document = DocumentId::new();
    let session = runtime
        .block_on(Session::memory(document, Bytes::new()))
        .expect("create blob benchmark project");
    let current = runtime.block_on(session.load()).expect("load state");
    let bytes = Bytes::from(vec![11; 4 * 1024 * 1024]);
    let blob = BlobId::for_bytes(&bytes);
    let next = current
        .update(
            current.revision().next().unwrap(),
            Bytes::new(),
            [blob],
            [(blob, bytes)],
        )
        .expect("derive blob state");
    let durable = runtime
        .block_on(session.save(&next))
        .expect("publish benchmark blob");
    criterion.bench_function("storage/mmap_blob_4m", |bencher| {
        bencher.iter(|| {
            black_box(
                runtime
                    .block_on(durable.blobs().get(black_box(blob)))
                    .expect("map blob"),
            )
        });
    });
}

criterion_group!(benches, storage_benchmarks);
criterion_main!(benches);
