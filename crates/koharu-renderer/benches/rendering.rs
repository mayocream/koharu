#![allow(unused)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use koharu_renderer::{
    font::FontBook,
    layout::{TextLayout, WritingMode},
    renderer::{RenderOptions, TinySkiaRenderer},
};

const FONT_SIZE: f32 = 24.0;
const SAMPLE_TEXT: &str = "The quick brown fox jumps over the lazy dog.";

fn rendering_benchmark(c: &mut Criterion) {
    let mut fontbook = FontBook::new();
    let renderer = TinySkiaRenderer::new().expect("Failed to create renderer");
    let post_script_name = fontbook
        .all_families()
        .into_iter()
        .find(|face| !face.post_script_name.is_empty())
        .map(|face| face.post_script_name)
        .expect("Failed to find font");
    let font = fontbook
        .query(&post_script_name)
        .expect("Failed to find font");
    let _ = font.fontdue().expect("Failed to load font");
    let layout = TextLayout::new(&font, Some(FONT_SIZE))
        .run(SAMPLE_TEXT)
        .expect("Failed to create layout");
    let options = RenderOptions {
        font_size: FONT_SIZE,
        ..Default::default()
    };

    c.bench_function("layout", |b| {
        b.iter(|| {
            let layout = TextLayout::new(&font, Some(FONT_SIZE))
                .run(black_box(SAMPLE_TEXT))
                .expect("Failed to create layout");
            black_box(layout);
        })
    });

    c.bench_function("render", |b| {
        b.iter(|| {
            let image = renderer
                .render(&layout, WritingMode::Horizontal, &options)
                .expect("Failed to render");
            black_box(image);
        })
    });
}

criterion_group!(benches, rendering_benchmark);
criterion_main!(benches);
