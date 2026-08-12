# koharu-canvas

`koharu-canvas` is Koharu's WGPU/Vello editor viewport. It presents a
rendered page, supports live editor previews, manages editable mask/raster
state, and returns validated commits to the application.

This document describes a behavior-preserving redesign. It simplifies
ownership without changing what users can see or do.

## Design constraints

- Keep the existing crate. Do not create a second canvas or rendering crate.
- `koharu-renderer` is the only owner of document rendering, font handling,
  image decoding for rendered layers, and retained vector nodes.
- `koharu-canvas` owns viewport state and transient interaction state. It does
  not reconstruct a second semantic page model.
- React continues to own tools, hit testing, selection, pointer gestures, and
  DOM controls.
- The application continues to own the scene session, commits, undo history,
  persistence, desktop window, and WGPU surface.
- Scene/resource operations are asynchronous. Drawing an already prepared
  frame stays synchronous inside the desktop frame loop.
- Refactoring does not silently add, remove, lock, or reinterpret an editor
  capability.

## Preserved behavior

The migration must preserve the current viewport behavior:

| Area | Required behavior |
| --- | --- |
| Page image | Preserve display of the source page image. Remove the unused flattened `rendered` page-view branch and its transition state. |
| Text | Draw translated text overlays only. Source OCR text remains editable document data and is never rendered on the canvas. |
| Images | Preserve page background, image entities, raster layers, geometry, order, visibility, and opacity. |
| Masks | Preserve active inpainting behavior through one transient scratch mask. There is no synthetic text/COO mask plane or role-derived persistent mask. |
| Raster editing | Preserve paint and erase previews, commit/acknowledgement flow, and the old image until replacement pixels are ready. |
| Transforms | Preserve text selection, move, resize, rotation, multi-selection, preview, cancellation, and commit behavior. Pixel/image layers never expose or accept width, height, or rotation transforms. |
| Viewport | Preserve camera conversion, fitted view, workspace color, resizing, damage tracking, and zero-sized viewport behavior. |
| Sampling | Preserve color sampling from the last successfully presented viewport image. |
| Revisions | Preserve contiguous revision checks, explicit reload after a gap, active-page removal, and unrelated-change fast paths. |
| Diagnostics | Preserve resource and rendering failures without replacing valid visible content prematurely. |

The redesign removes the unused special `rendered`, `text-mask`, and `coo-mask`
canvas paths, their display state, resource scheduling, commands, and tests.
The source page image remains the page background. Additional color images and
raster edits use ordinary scene layers. Inpainting keeps an application-owned
transient scratch mask until its result is committed. A future persistent mask
must be an explicit scene layer with real editable pixel/channel metadata; the
canvas must not infer one from asset roles or reintroduce special page planes.

## Ownership

### Application

The application owns:

- `koharu-scene::Session`, the current snapshot, and undo history;
- one long-lived `koharu_renderer::Renderer`;
- one `Canvas` for the desktop viewport;
- conversion of `TransformCommit`, raster commits, and mask commits into one
  atomic scene patch;
- WGPU surface acquisition and presentation beneath the transparent WebView.

The application is the only component that commits persistent changes.

### Renderer

The renderer turns a snapshot into an immutable `Frame`. The frame already
contains ordered document layers, text layout, image content, visibility,
opacity, entity lookup, and vector scenes. Canvas never repeats that traversal
or shaping work.

Canvas receives the renderer's complete frame. The frame retains authored
presentation metadata so canvas can apply interactive opacity and visibility
without a second document-rendering path. Translation-only rendering is a
renderer invariant, not a canvas option. There is no source-fallback flag or
canvas-specific branch.

### Canvas

Canvas owns only viewport and transient editor state:

- the current immutable renderer `Frame`;
- camera and physical viewport size;
- damage state and the offscreen Vello target;
- transient transforms and opacity previews;
- active raster strokes and their retained preview scene;
- sparse editable masks and pending acknowledgements;
- asynchronous color-sample requests;
- access to diagnostics retained by the current renderer frame.

Canvas does not own a `Snapshot`, persistent hierarchy, text semantics, font
library, general decoded-image cache, or undo history.

## Public flow

The host creates and retains one renderer and one canvas. Rendering and scene
synchronization stay on the application-owned async path; canvas installation
is a complete synchronous swap:

```rust,ignore
let renderer = koharu_renderer::Renderer::new()?;
let mut canvas = Canvas::new(gpu, wake)?;

let frame = renderer.render(&snapshot, page).await?;
canvas.set_frame(frame)?;
```

`Renderer::render` prepares all data needed to display the selected page before
the application replaces the current frame. Failure leaves the previous valid
frame visible. Blob reads, decoding, and font loading do not run on the desktop
event-loop thread.

After a scene commit:

```rust,ignore
let commit = session.commit(patch).await?;
let frame = renderer
    .update(canvas.frame().expect("active frame"), &commit.snapshot, &commit.changes)
    .await?;
canvas.set_frame(frame)?;
```

`Renderer::update` rejects a revision gap so the application can call
`Renderer::render` explicitly. Removing the active page calls `Canvas::clear`,
which retains reusable GPU objects.

The desktop frame loop remains synchronous:

```rust,ignore
if canvas.needs_redraw() {
    let frame = canvas.render()?;
    presenter.present(frame)?;
}
```

`render` performs no scene reads, font loading, image decoding, or blocking GPU
wait. It only composes retained content and transient previews into the
viewport-sized target.

## Rendering path

```text
Snapshot commit
      |
      v
Renderer::render/update (async) -> immutable Frame
      |
      v
Canvas stores Frame + transient editor state
      |
      v
damage-tracked Vello scene -> viewport texture
      |
      v
desktop presenter -> WGPU surface beneath transparent WebView
```

When there is no active transform, mask change, raster preview, or
presentation override, canvas appends the retained frame directly. Interactive
updates reuse per-layer vector scenes and change only placement/presentation.

Canvas renders strictly at viewport size. Page size and zoom affect the camera,
not the GPU target dimensions. A page larger than the viewport therefore does
not allocate a page-sized interactive texture.

## Interaction contract

React supplies absolute page-space preview frames. Canvas validates identity,
visibility, complete selection membership, finite values, monotonic frame
numbers, and the capabilities defined by the application.

```rust,ignore
canvas.begin_transform(&selection)?;
canvas.update_transform(frame_number, &complete_preview)?;

if let Some(commit) = canvas.finish_transform()? {
    // The application validates document policy and commits all geometry in
    // one scene patch, then calls Canvas::sync with the resulting Change.
}
```

Stale transform frames are ignored. Partial or duplicate frames are rejected.
Cancellation restores retained content without a scene commit. Canvas returns
geometry; it does not mutate the snapshot itself.

Raster previews retain only accepted stroke segments. Mask storage remains
sparse and tiled: empty tiles are not allocated, fully erased tiles are
released, dirty bounds contain only changed pixels, and snapshots copy only
occupied tiles.

## Resources and concurrency

- Renderer owns document image/font resources used by a `Frame`.
- Canvas owns only resources intrinsic to transient editing, such as sparse
  writable masks, raster previews, and reusable viewport/readback textures.
- Resource work is generation-tagged; stale completion cannot replace newer
  page or edit state.
- Worker counts and queues are bounded. Pointer handling and presentation never
  wait for decode workers.
- Repeated synchronization with the same revision/resources does not cancel or
  reschedule useful work.
- GPU readback uses reusable buffers and non-blocking polling; resize and clear
  cancel outstanding user requests deterministically.

## Damage model

Damage is divided by ownership:

- target damage: viewport size or GPU target changed;
- content damage: renderer frame, camera, presentation, mask, or
  interactive preview changed;
- polling only: an asynchronous sample is pending but visible content is
  unchanged.

Unchanged generations do not acquire the surface or submit a new visible
frame. DOM-only selection controls never invalidate Vello content.

## Module boundaries

The target modules are small owners, not lifecycle wrappers:

| Module | Responsibility |
| --- | --- |
| `canvas` | Public facade, current frame installation, and render orchestration. |
| `geometry` | Camera, coordinate, frame, and bounds math. |
| `transform` | Validated transient transform state and commits. |
| `raster` | Paint/erase stroke state and retained raster preview. |
| `mask` | Sparse tiled masks, stroke state, snapshots, and commit generations. |
| `gpu` | Viewport target, Vello rendering, sampling ring, and readback polling. |
| `damage` | Target/content/poll invalidation. |
| `error` | Canvas-domain errors. |

There is no independent canvas `model`, document `resources` cache, or
renderer adapter. Those would duplicate scene or renderer ownership.

## Migration and verification

The canvas migration must not be combined with UI behavior changes:

1. Characterize active source-page display, text policy, image order,
   transforms, raster strokes, inpainting, sampling, revisions, and damage
   behavior.
2. Introduce renderer `Frame` consumption while the old tests still define the
   expected behavior.
3. Move resource ownership to the renderer without changing visual selection
   or readiness semantics.
4. Update desktop and React callers directly; add no compatibility facade.
5. Delete duplicated canvas model/resource/rendering code only after focused
   desktop and UI tests pass.
6. Compare representative desktop screenshots and sampled pixels before and
   after the migration.
7. Delete the unused rendered-page view, transition, text-mask, and COO-mask
   branches instead of porting them.

Focused validation:

```text
cargo test -p koharu-canvas --lib
cargo check -p koharu-canvas --all-targets
```

Run real-GPU visual tests separately on a provisioned adapter. Validate the
final canvas/WebView composition through the desktop window.
