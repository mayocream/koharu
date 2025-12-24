#![allow(unused)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use koharu_renderer::{
    font::{FamilyName, FontBook, Properties},
    layout::{TextLayout, WritingMode},
    renderer::{RenderOptions, WgpuRenderer},
};

const FONT_SIZE: f32 = 24.0;
const SAMPLE_TEXT: &str = "The quick brown fox jumps over the lazy dog.";

fn rendering_benchmark(c: &mut Criterion) {
    let mut fontbook = FontBook::new();
    let renderer = WgpuRenderer::new().expect("Failed to create renderer");
    let font = fontbook
        .query(&[FamilyName::SansSerif], &Properties::default())
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
