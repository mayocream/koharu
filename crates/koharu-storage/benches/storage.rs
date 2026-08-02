use std::{collections::BTreeSet, hint::black_box};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use koharu_storage::{CommitRequest, DocumentId, Revision, Session};

fn storage_benchmarks(criterion: &mut Criterion) {
    criterion.bench_function("storage/opaque_commit", |bencher| {
        bencher.iter_batched(
            || {
                let document = DocumentId::new();
                let session = Session::memory(document, Vec::new()).unwrap();
                let request =
                    CommitRequest::new(document, Revision::ZERO, vec![1; 64], vec![2; 64]);
                (session, request)
            },
            |(mut session, request)| {
                black_box(session.commit(request, None, BTreeSet::new()).unwrap());
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, storage_benchmarks);
criterion_main!(benches);
