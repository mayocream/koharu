use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use koharu_scene::{
    CanvasSize, CommandBatch, Page, Parent, Position, Session, SessionConfig, node,
};

fn populated_scene(nodes: usize) -> (Session, koharu_scene::PageId, koharu_scene::NodeId) {
    let config = SessionConfig {
        checkpoint_interval: None,
        max_nodes: nodes,
        max_commands_per_batch: nodes + 1,
        ..SessionConfig::default()
    };
    let mut session = Session::memory(config).expect("create in-memory scene");
    let mut commands = CommandBatch::new(session.revision());
    let page = commands
        .create_page(Page::new("Benchmark", CanvasSize::new(4096, 4096)))
        .expect("create page");
    let mut target = None;
    for _ in 0..nodes {
        let node = commands
            .create(Parent::Page(page), Position::Top, node::group())
            .expect("create node");
        target.get_or_insert(node);
    }
    session.apply(commands).expect("populate scene");
    (session, page, target.expect("at least one benchmark node"))
}

fn scene_benchmarks(c: &mut Criterion) {
    let mut traversal = c.benchmark_group("scene_walk");
    for nodes in [1_000, 10_000, 100_000] {
        let (session, page, _) = populated_scene(nodes);
        traversal.bench_with_input(BenchmarkId::from_parameter(nodes), &nodes, |b, _| {
            b.iter(|| {
                let count = session
                    .scene()
                    .page(page)
                    .expect("benchmark page")
                    .walk()
                    .count();
                black_box(count)
            });
        });
    }
    traversal.finish();

    let (mut session, _, target) = populated_scene(100_000);
    let mut opacity = 0.5;
    let mut scalar = c.benchmark_group("scalar_commit_100k_scene");
    scalar.sample_size(20);
    scalar.measurement_time(Duration::from_secs(2));
    scalar.bench_function("set_opacity", |b| {
        b.iter(|| {
            let mut commands = CommandBatch::new(session.revision());
            commands
                .set_opacity(target, opacity)
                .expect("build opacity command");
            let applied = session.apply(commands).expect("commit opacity");
            opacity = if opacity == 0.5 { 0.75 } else { 0.5 };
            black_box(applied);
        });
    });
    scalar.finish();
}

criterion_group!(benches, scene_benchmarks);
criterion_main!(benches);
