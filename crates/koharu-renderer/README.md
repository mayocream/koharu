# koharu-renderer

`koharu-renderer` turns a `koharu-scene` snapshot into reusable vector content
and, when requested, CPU-readable pixels. It is shared by the editor canvas,
PNG export, and PSD export.

The crate has four explicit operations:

```text
Snapshot + RenderRequest
             |
             v
Compositor::compile
             |
             v
       Composition
             |
             v
SceneRenderer::render ------> TextRenderer::layout / render
             |
             v
           Frame --------------> embed in another Vello scene
             |
             v
Rasterizer::rasterize
             |
             v
           Raster
```

There is deliberately no all-in-one renderer facade. Callers own the
components they use and can stop at `Composition`, `Frame`, or `Raster`.

## Responsibilities

### `Compositor::compile`

The compositor reads semantic scene components and produces a `Composition`.
It resolves layer order, the single translation and source-text fallback,
writing mode, alignment, bubble constraints, visibility, and asset roles.

A `Composition` is backend-independent. It contains no decoded images, font
handles, Vello commands, GPU state, or raster settings. It records the scene
revision plus the entity, relation, and blob dependencies used to produce it.

Compilation is read-only and deterministic for a snapshot and request.

### `TextRenderer::layout` and `TextRenderer::render`

`TextRenderer` owns the text-specific boundary:

- `layout` shapes and lays out text through `TextLayout`;
- `render` records a `LayoutRun` into a caller-owned `vello::Scene`;
- the scene renderer uses the same component for font fallback, auto-fit,
  bubble-safe layout, overflow diagnostics, rotation, fill, and stroke policy.

Text regions constrain layout and auto-fit but never clip painted glyphs. This
keeps authored overflow visible and editable while still reporting it through
`RenderDiagnostic::TextOverflow`.

This keeps direct text rendering available without making the compositor or
rasterizer understand glyphs.

### `SceneRenderer::render`

The scene renderer resolves a `Composition` into a retained vector `Frame`.
It owns decoded-image and font caches, delegates text to `TextRenderer`, records
independent layers in parallel, and caches frames by scene revision, font
generation, and render request. System fonts are discovered once. Koharu's
bundled-font catalog stays resident as metadata, while individual faces are
downloaded on first use and retained by a byte-bounded LRU cache.

A `Frame` owns an immutable Vello scene and downstream metadata:

- page and revision;
- vector surface size and page-space origin;
- ordered visual layers;
- resolved editable-text information for PSD export;
- dependencies and diagnostics;
- per-entity vector scenes for editor transforms and cropped exports.

`Frame::append_to` embeds the whole frame in another Vello scene.
`Frame::append_entity_to` embeds one entity without rasterization.
`Frame::entity` returns a tightly cropped vector frame for one entity; the
rasterizer therefore never needs target-selection logic.

### `Rasterizer::rasterize`

The rasterizer converts a `Frame` into a `Raster`. `Raster` contains the RGBA
image and its page-space origin, so a page frame and an entity frame use the
same API.

`Rasterizer` owns the headless Vello renderer, device context, readback
buffers, and a small reusable target pool. `RasterOptions` controls only
supersampling and downsampling and is intentionally absent from
`RenderRequest`, `Composition`, and `Frame`.

The crate does not depend on `wgpu` directly. Vello selects the compatible WGPU
version and `koharu-renderer` uses `vello::wgpu` for the backend types needed by
Vello rasterization.

## Page rendering

```rust,ignore
use koharu_renderer::{
    Compositor, RasterOptions, Rasterizer, RenderRequest, SceneRenderer,
};

let request = RenderRequest::new(page);

let compositor = Compositor::new();
let composition = compositor.compile(&snapshot, &request)?;

let scene_renderer = SceneRenderer::new();
let frame = scene_renderer.render(&snapshot, &composition)?;

let rasterizer = Rasterizer::new()?;
let raster = rasterizer.rasterize(&frame, RasterOptions::default())?;
raster.image.save("page.png")?;
```

Long-lived callers should keep `SceneRenderer` and `Rasterizer` alive. Reusing
them preserves decoded/font resources, retained frames, Vello caches, and GPU
targets.

## Vector embedding

The canvas stops at `Frame` and appends its vector content directly:

```rust,ignore
let mut scene = vello::Scene::new();
frame.append_to(&mut scene, None);

frame.append_entity_to(entity, &mut scene, Some(preview_transform));
```

This avoids GPU readback during interactive rendering and allows transient
entity transforms without repeating image decoding or text layout.

## Entity rasterization

PSD export uses the same frame as PNG export, then derives cropped vector
frames for editable text-layer previews:

```rust,ignore
if let Some(entity_frame) = frame.entity(entity)? {
    let layer = rasterizer.rasterize(&entity_frame, raster_options)?;
    // layer.left and layer.top are page-space PSD offsets.
}
```

Cropping happens before rasterization. The GPU only sees the smaller entity
surface, and `Rasterizer` remains independent of scene entities and PSD rules.

## Direct text rendering

```rust,ignore
use koharu_renderer::{
    FontSystem, TextLayout, TextRenderOptions, TextRenderer, WritingMode,
};
use vello::{Scene, kurbo::Affine};

let font = FontSystem::new().first_font()?;
let builder = TextLayout::new(&font)
    .with_font_size(24.0)
    .with_max_width(640.0);

let renderer = TextRenderer::new();
let layout = renderer.layout(&builder, "Hello")?;
let mut scene = Scene::new();
renderer.render(
    &mut scene,
    &layout,
    WritingMode::Horizontal,
    &TextRenderOptions::default(),
    Affine::IDENTITY,
);
```

`TextRenderOptions` contains glyph-paint settings only. Surface background,
supersampling, and readback policy belong to `Rasterizer`.

## Requests and diagnostics

`RenderRequest` selects one page and the non-persistent visual policy used to
compile and render it:

- whether source text may be used when a text content entity has no
  `Translation` component;
- base image roles in preference order;
- child image role and whether image entities are included;
- `RenderTheme` for fonts, sizing, spacing, insets, alignment, color, stroke,
  and auto-fit.

The renderer resolves typed `Presents` and `FitsTo` relations from the manga
schema. Relation names and endpoint rules are not render-request policy.

Compilation and rendering report non-fatal decisions through
`RenderDiagnostic`, including missing base assets, source-text fallback,
overflow, and text below the configured readability floor.

## Caching and invalidation

`SceneRenderer` caches a bounded number of immutable `Arc<Frame>` values. A
cache key contains the scene revision, font generation, and complete
`RenderRequest`. Raster options do not affect that key because they are not
part of vector rendering.

After committing a project patch, pass the matching `Change` to
`SceneRenderer::apply_changes`. Frames whose recorded dependencies were not
changed are advanced to the new revision; affected frames are discarded.
`clear_cache` is available for explicit full invalidation.

Loading a bundled face increments the font generation. A later render uses a
new frame key without requiring callers to recreate the scene renderer.

## Concurrency

- `Compositor` and `TextRenderer` are stateless and cheap to create.
- `SceneRenderer` shares resources and serializes only its frame-cache access;
  independent layer recording uses Rayon.
- `Frame` is immutable after construction and is normally shared through
  `Arc`.
- `Rasterizer` serializes Vello command encoding because Vello mutates internal
  caches. GPU completion and readback happen after releasing that lock.

## Limits

Render requests reject non-finite or non-positive page dimensions, dimensions
above 32,768 pixels, and surfaces above 268,435,456 pixels. Supersampling is
clamped to a factor of four and checked for integer overflow.

Decoded assets must match their declared dimensions and byte count. Image
decode, font resolution, text layout, scene access, and backend errors retain
their source errors in the public error type.

## Module map

| Module | Responsibility |
| --- | --- |
| `compositor` | Scene semantics, `Composition`, dependencies, diagnostics. |
| `text_renderer` | Text layout dispatch and Vello glyph recording. |
| `scene_renderer` | Image resolution, vector layer recording, frame cache. |
| `rasterizer` | Vello GPU execution, readback, target reuse, downsampling. |
| `request` | `RenderRequest`, `RenderTheme`, rendering policy. |
| `fonts` | System discovery, bundled catalog, fallback, and bounded lazy loading. |
| `layout`, `shape`, `segment`, `script` | Unicode shaping and layout engine. |
| `bubble` | Region association and balloon-safe layout geometry. |

## Extension rules

- Scene interpretation belongs in `Compositor`.
- Text shaping and glyph recording belong in `TextRenderer`.
- Resource-backed vector construction belongs in `SceneRenderer`.
- Pixel production and GPU details belong in `Rasterizer`.
- Canvas composition consumes `Frame`; it does not duplicate renderer policy.
- Export-format metadata and serialization stay outside this crate.
- Do not add raster settings to `RenderRequest` or target-selection enums to
  `Rasterizer`; the scene owns one translation per text entity.

## Validation

```text
cargo test -p koharu-renderer
cargo check -p koharu-renderer --all-targets
cargo clippy -p koharu-renderer --all-targets -- -D warnings
```

GPU visual tests are ignored by default. Run them on a machine with a usable
Vello adapter:

```text
cargo test -p koharu-renderer --test rendering -- --ignored
```
