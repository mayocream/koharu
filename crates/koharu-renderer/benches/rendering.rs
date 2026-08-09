use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use koharu_renderer::{
    FontSystem, RasterOptions, Rasterizer, TextLayout, TextRenderOptions, TextRenderer, WritingMode,
};
use vello::{Scene, kurbo::Affine};

const FONT_SIZE: f32 = 24.0;
const SAMPLE_TEXT: &str = "The quick brown fox jumps over the lazy dog.";

fn rendering_benchmark(c: &mut Criterion) {
    let mut fonts = FontSystem::new();
    let text_renderer = TextRenderer::new();
    let rasterizer = Rasterizer::new().expect("failed to create rasterizer");
    let font = fonts.first_font().expect("failed to find font");
    let layout = TextLayout::new(&font)
        .with_font_size(FONT_SIZE)
        .run(SAMPLE_TEXT)
        .expect("failed to create layout");
    let options = TextRenderOptions::default();

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
            let mut scene = Scene::new();
            text_renderer.render(
                &mut scene,
                &layout,
                WritingMode::Horizontal,
                &options,
                Affine::IDENTITY,
            );
            let image = rasterizer
                .rasterize_scene(
                    &scene,
                    layout.width.ceil() as u32,
                    layout.height.ceil() as u32,
                    [0, 0, 0, 0],
                    RasterOptions::default(),
                )
                .expect("failed to render");
            black_box(image);
        })
    });
}

criterion_group!(benches, rendering_benchmark);
criterion_main!(benches);
