# koharu-canvas

`koharu-canvas` is the interactive, WGPU-backed editor viewport for a
`koharu-scene` snapshot. It draws an editable page, text and image entities,
masks, selection chrome, and live transform previews into an offscreen texture.
The application owns the window, surface, scene session, undo history, and
persistence.

The canvas is a view and interaction engine, not another scene model. Its
internal page representation is derived and disposable; committed data remains
in `koharu-scene`.

## Scene contract

`Canvas::show_page` consumes an immutable `SceneSnapshot` and the page's
`EntityId`. `PageId` and `ElementId` are canvas aliases for that same unified
identifier.

An editable page uses these scene components:

| Owner | Component or asset slot | Meaning |
| --- | --- | --- |
| page entity | `Page` | label and page-space dimensions |
| page entity | asset `source` | required original page image |
| page entity | asset `clean` | optional inpainted editable base |
| page entity | asset `rendered` | optional flattened preview |
| page entity | asset `text-mask` | optional segmentation mask |
| page entity | asset `brush-mask` | optional user repair mask |
| page descendants | `Geometry` | editable polygon and drawing bounds |
| page descendants | `Visibility` | visibility and opacity |
| page descendants | asset `source` | optional image layer |
| page descendants | `SourceText` | editable OCR data and text-block identity; never drawn as a viewport overlay |
| page descendants | locale-keyed `Translation` and typography components | optional viewport text layer |

Descendants are drawn in subtree order. Four-point rectangular geometries keep
their rotation in the editor; arbitrary polygons currently use their
axis-aligned bounds for hit testing and image placement. The original polygon
is retained and transformed on commit.

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

# fn example(
#     device: Arc<wgpu::Device>,
#     queue: Arc<wgpu::Queue>,
#     wake: Arc<dyn Fn() + Send + Sync>,
#     snapshot: &koharu_scene::SceneSnapshot,
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

The render path has three damage-tracked stages:

1. Resize or recreate the viewport-sized GPU target.
2. Compose stable page content only when scene data, decoded resources,
   display mode, text policy, or an interactive preview changes.
3. Compose inexpensive editor overlays when the camera, selection, handles,
   guides, cursor, or draft bounds change.

Every text block always uses fixed screen-space editor chrome: a translucent
rose rounded rectangle and a numbered circular indicator at the top-left
corner. The 32 px indicator is tangent to the outside of the block so it never
covers text, and its number is antialiased with a platform sans-serif font.
Selected bounds use the stronger primary rose treatment and expose visible
resize and rotation handles, except that the numbered indicator owns the
north-west corner instead of a resize handle.

Image bytes are read from the snapshot and decoded off the event-loop thread.
Decoded images use an LRU budget controlled by `CanvasOptions::max_decoded_bytes`.
Masks stay as single-channel 256×256 copy-on-write tiles, so a stroke clones
only the tiles it touches.

Text uses the same `RenderPlan`, `PreparedPage`, `RenderResources`, and
`RenderTheme` pipeline as `koharu-renderer`. The canvas asks it to prepare
transparent text-only layers and reuses per-entity Vello scenes for live
transforms. This avoids duplicated shaping policy and avoids GPU readback.
The interactive canvas requires a target locale and disables source-text
fallback: OCR text remains editable scene data but is never drawn as an editor
overlay when a translation is missing.
`set_text_options`, `set_locale`, and `invalidate_fonts` explicitly invalidate
prepared text.

`Canvas::render` returns a borrowed `CanvasFrame` containing an offscreen
`TextureView`. The host presents that texture to its window surface. A zero-size
viewport is valid, and `needs_redraw` is true only while a bounded transition is
active.

## Interaction and commits

Coordinate conversion, rotated hit testing, handles, move/resize/rotate
previews, mask painting, and cancellation are owned by the canvas. Persistent
mutation is owned by the application.

Only text blocks and child image entities participate in editor hit testing.
Detection-only panel, bubble, and other analysis regions remain scene data for
pipeline and rendering decisions but cannot be selected or transformed.

Element transforms follow this flow:

```rust,ignore
canvas.begin_transform(&selection, target, pointer)?;
canvas.update_transform(pointer)?;

if let Some(commit) = canvas.finish_transform()? {
    let patch = session.snapshot().patch(|edit| {
        for element in &commit.elements {
            edit.set(element.element, "default", &element.geometry)?;
        }
        Ok(())
    })?;
    let committed = session.commit(patch)?;
    canvas.sync(&committed.snapshot, &committed.changes)?;
}
```

A `TransformCommit` returns complete transformed `Geometry` values, preserving
component origin and arbitrary polygon points. The host can include additional
domain updates in the same atomic patch.

Mask strokes return a `MaskCommit`. `encode_png` produces a grayscale PNG that
the host stores using `AssetRole::new(commit.plane.slot())`. Once the scene
commit supplies the new `BlobId`, call `acknowledge_mask_commit` with the mask
generation. Generation checks prevent an older asynchronous save from
overwriting newer local mask edits. Incoming mask replacement conflicts with
uncommitted local edits instead of silently discarding them.

## Modules

- `canvas`: public facade, revision synchronization, damage orchestration
- `model`: immutable active-page materialization from a scene snapshot
- `geometry`: camera and coordinate types, frames, hit-test math
- `transform`: pure move, resize, rotate, preview, and geometry commit logic
- `mask`: copy-on-write tiled grayscale masks and stroke state
- `resources`: asynchronous decode and byte-budgeted image cache
- `elements`: renderer-backed text preparation and ordered Vello composition
- `overlay`: editor chrome geometry and composition
- `gpu`: offscreen WGPU targets and render passes
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
