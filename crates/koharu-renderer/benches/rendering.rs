use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use koharu_renderer::{FontSystem, RenderOptions, TextLayout, WgpuRenderer, WritingMode};

const FONT_SIZE: f32 = 24.0;
const SAMPLE_TEXT: &str = "The quick brown fox jumps over the lazy dog.";

fn rendering_benchmark(c: &mut Criterion) {
    let mut fonts = FontSystem::new();
    let renderer = WgpuRenderer::new().expect("failed to create renderer");
    let font = fonts.first_font().expect("failed to find font");
    let layout = TextLayout::new(&font)
        .with_font_size(FONT_SIZE)
        .run(SAMPLE_TEXT)
        .expect("failed to create layout");
    let options = RenderOptions {
        font_size: FONT_SIZE,
        ..Default::default()
    };

    c.bench_function("layout", |b| {
        b.iter(|| {
            let layout = TextLayout::new(&font)
                .with_font_size(FONT_SIZE)
                .run(black_box(SAMPLE_TEXT))
                .expect("failed to create layout");
            black_box(layout);
        })
    });

    c.bench_function("render", |b| {
        b.iter(|| {
            let image = renderer
                .render(&layout, WritingMode::Horizontal, &options)
                .expect("failed to render");
            black_box(image);
        })
    });
}

criterion_group!(benches, rendering_benchmark);
criterion_main!(benches);
