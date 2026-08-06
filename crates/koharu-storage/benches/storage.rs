use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use koharu_storage::{Commit, DocumentId, Revision, Session};

fn commits(criterion: &mut Criterion) {
    criterion.bench_function("rocksdb/atomic_scene_commit", |bencher| {
        bencher.iter_batched(
            || {
                let document = DocumentId::new();
                let session = Session::memory(document, Vec::new()).unwrap();
                let commit = Commit::new(document, Revision::ZERO, vec![1; 512], vec![2; 512]);
                (session, commit)
            },
            |(mut session, commit)| black_box(session.commit(commit, None).unwrap()),
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, commits);
criterion_main!(benches);
