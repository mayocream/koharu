# koharu-canvas

`koharu-canvas` is the Vello-backed editor viewport for a `koharu-scene`
snapshot. It draws an editable page, text and raster-layer entities, masks,
and live transform previews into an offscreen texture. React owns tools, hit
testing, selection, pointer gestures, and DOM controls. The canvas receives
semantic preview operations and owns their validated native rendering. The
application owns the window, surface, scene session, undo history, and
persistence.

The canvas is a view and interaction engine, not another scene model. Its
internal page representation is derived and disposable; committed data remains
in `koharu-scene`.

## Scene contract

`Canvas::show_page` consumes an immutable `Snapshot` and the page's
`EntityId`. `PageId` and `ElementId` are canvas aliases for that same unified
identifier.

An editable page uses these scene components:

| Owner | Component or asset role | Meaning |
| --- | --- | --- |
| page entity | `Page` | label and page-space dimensions |
| page entity | asset `source` | required original page image |
| page entity | asset `rendered` | optional flattened preview |
| page entity | asset `text-mask` | optional segmentation mask |
| page descendants | `Geometry` | editable polygon and text bounds |
| page descendants | `RasterLayer` plus asset `source` | ordered transparent Cleanup and Paint layers |
| page descendants | `Visibility` | visibility and opacity |
| page descendants | asset `source` | optional image or raster-layer pixels |
| content entity | `TextContent`, `SourceText`, and optional `Translation` | semantic OCR and translation data; never geometry or presentation |
| text-layer entity | `TextLayout`, optional `Typography`, and `Presents` relation | editable viewport text presentation |
| analysis-region entity | `Region`, `Geometry`, and analysis components | source-artwork observations used by OCR and fitting |

Descendants are drawn in subtree order. Four-point rectangular geometries keep
their rotation in the editor; arbitrary polygons currently use their
axis-aligned bounds for React hit testing and image placement. The original
polygon is retained and transformed on commit.

Page dimensions are converted to physical-size integers for raster resources.
The current safety ceiling is 32,768 pixels per side and 268,435,456 pixels per
page. A page without a `source` asset is rejected before replacing the active
page.

## Ownership and lifecycle

The host creates one GPU device and queue and shares them with the canvas. A
wake callback lets background image decoding request another event-loop turn.

```rust,no_run
use std::sync::Arc;
use koharu_canvas::{Canvas, CanvasGpu};
use vello::wgpu;

# fn example(
#     device: Arc<wgpu::Device>,
#     queue: Arc<wgpu::Queue>,
#     wake: Arc<dyn Fn() + Send + Sync>,
#     snapshot: &koharu_scene::Snapshot,
#     page: koharu_scene::EntityId,
# ) -> koharu_canvas::Result<()> {
let mut canvas = Canvas::new(CanvasGpu { device, queue }, wake)?;
canvas.show_page(snapshot, page)?;
# Ok(())
# }
```

After every successful scene commit, pass its matching snapshot and change set
to the canvas:

```rust,ignore
let committed = session.commit(patch)?;
canvas.sync(&committed.snapshot, &committed.changes)?;
```

Revisions must be contiguous. A gap returns `Error::RevisionConflict` so the
host can recover explicitly with `show_page`. Removing the active page clears
the viewport. Unrelated changes only advance the retained snapshot; changes
that may affect the active subtree rebuild its derived page and render caches.

`clear_page` releases the active page state. It does not mutate the session.

## Rendering

The render path has two damage-tracked stages:

1. Resize or recreate the Vello GPU target.
2. Compose page content only when scene data, decoded resources, display mode,
   camera, text policy, masks, or an interactive preview changes.

Selection outlines, text indicators, guides, cursors, and resize/rotation
handles are transparent DOM overlays. They no longer invalidate or traverse
the Vello scene.

Image bytes are read from the snapshot and decoded off the event-loop thread.
Decoded images use an LRU budget controlled by `CanvasOptions::max_decoded_bytes`.
Masks stay as single-channel 256×256 copy-on-write tiles, so a stroke clones
only the tiles it touches.

Text uses the same `Compositor`, `SceneRenderer`, and `RenderTheme` pipeline as
`koharu-renderer`. The canvas compiles a transparent
text-only `Composition`, renders it to a retained `Frame`, and reuses its
per-entity Vello scenes for live transforms. This avoids duplicated shaping
policy and GPU readback.
The interactive canvas renders text-layer entities whose related content has a
`Translation` component, and disables source-text fallback: OCR text remains
editable scene data but is never drawn as an editor overlay when a translation
is missing. `set_text_options` and
`invalidate_fonts` explicitly invalidate the retained text frame.

`Canvas::render` returns a borrowed `CanvasFrame` containing an offscreen
`TextureView`. The host presents that texture with Vello's `TextureBlitter`.
A zero-size viewport is valid, and `needs_redraw` is true only while a bounded
transition is active.

## Interaction and commits

React owns rotated hit testing, selection policy, handles, and move/resize/
rotate geometry. The canvas validates and renders absolute preview frames;
mask rasterization and color sampling remain native. Persistent mutation is
owned by the application. The canvas has no tool, pointer-button, or pointer-
phase state.

Only text blocks and child image entities participate in React editor hit testing.
Detection-only panel, bubble, and other analysis regions remain scene data for
pipeline and rendering decisions but cannot be selected or transformed.

Element transforms follow this flow:

```rust,ignore
canvas.begin_transform(&selection)?;
canvas.update_transform(
    frame_number,
    &[koharu_canvas::ElementFrame { element, frame }],
)?;

if let Some(commit) = canvas.finish_transform()? {
    let patch = session.snapshot().patch(|edit| {
        for element in &commit.elements {
            edit.set(element.element, &element.geometry)?;
        }
        Ok(())
    })?;
    let committed = session.commit(patch)?;
    canvas.sync(&committed.snapshot, &committed.changes)?;
}
```

Each monotonically numbered update must contain the complete selected element
set. Stale frames are ignored, invalid or partial frames are rejected, and an
unchanged frame does not trigger Vello work. A `TransformCommit` returns
complete transformed `Geometry` values, preserving component origin and
arbitrary polygon points. The host can include additional domain updates in
the same atomic patch.

Mask strokes return a `MaskCommit`. `encode_png` produces a grayscale PNG that
the host stores using `AssetRole::new(commit.plane.slot())`. Once the scene
commit supplies the new `BlobId`, call `acknowledge_mask_commit` with the mask
generation. Generation checks prevent an older asynchronous save from
overwriting newer local mask edits. Incoming mask replacement conflicts with
uncommitted local edits instead of silently discarding them.

## Modules

- `canvas`: public facade, revision synchronization, damage orchestration
- `model`: immutable active-page materialization from a scene snapshot
- `geometry`: camera, coordinate, and frame types
- `transform`: validated absolute previews and geometry commit logic
- `mask`: copy-on-write tiled grayscale masks and stroke state
- `resources`: asynchronous decode and byte-budgeted image cache
- `elements`: renderer-backed text preparation and ordered Vello composition
- `gpu`: the offscreen Vello target
- `damage`: render-stage invalidation

Most behavioral tests are GPU-independent. The real-GPU visual test is ignored
by default and can be run on a machine with a provisioned adapter:

```text
cargo test -p koharu-canvas -- --ignored
```

The normal package validation is:

```text
cargo test -p koharu-canvas --lib
cargo check -p koharu-canvas --all-targets
cargo clippy -p koharu-canvas --all-targets -- -D warnings
```
