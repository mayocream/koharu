# koharu-renderer

Text layout and rasterization utilities used to place translated text back onto manga pages.

- `FontBook`: loads system fonts, filters by language/family, and caches face data.
- `Layouter`: shapes text with swash, supports horizontal and vertical flow, and applies OpenType vertical features when needed.
- `Renderer`: draws glyph runs into an `RgbaImage` with subpixel and color glyph support plus blending helpers.
- `types`: simple aliases for colors and points shared across the renderer.

## Example
```rust
use koharu_renderer::{
    Script,
    font::FontBook,
    layout::{LayoutRequest, Layouter, Orientation},
    render::{RenderRequest, Renderer},
};

let mut fontbook = FontBook::new();
let fonts = fontbook.filter_by_families(&["Noto Sans".to_string()]);

let mut layouter = Layouter::new();
let layout = layouter.layout(&LayoutRequest {
    text: "Hello world",
    fonts: &fonts,
    font_size: 28.0,
    line_height: 1.2,
    script: Script::Latin,
    max_primary_axis: 400.0,
    direction: Orientation::Horizontal,
})?;

let mut canvas = image::RgbaImage::new(512, 512);
let mut renderer = Renderer::new();
renderer.render(&mut RenderRequest {
    layout: &layout,
    image: &mut canvas,
    x: 24.0,
    y: 48.0,
    font_size: 28.0,
    color: [0, 0, 0, 255],
    direction: Orientation::Horizontal,
})?;
```

## License

Licensed under Apache-2.0.
