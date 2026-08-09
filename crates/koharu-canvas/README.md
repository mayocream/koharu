# koharu-canvas

This document defines the target architecture for `koharu-canvas`. During the
migration, implementation details may temporarily differ from this contract.
Replaced APIs must be removed rather than preserved through compatibility
layers.

`koharu-canvas` is the native interactive viewport for an already composed
page. It combines immutable renderer content with transient editor previews and
presents the result through WGPU beneath Koharu's transparent WebView.

The canvas is not a scene reader and not a second renderer.

```text
koharu-scene Snapshot
        |
        |  application-owned async orchestration
        v
koharu-renderer Renderer
        |
        v
    Composition ---------> koharu-canvas Canvas
                                  |
                                  v
                         viewport-sized WGPU texture
```

## Ownership

The canvas owns:

- the currently installed immutable `Composition`;
- camera and viewport state;
- damage tracking;
- transient transform and raster-stroke previews;
- sparse editable mask tiles and their local generations;
- viewport-sized Vello/WGPU targets;
- asynchronous color-sample readback slots;
- composition of retained page layers with native transient overlays.

The canvas does not own:

- a `Snapshot` or derived semantic page model;
- hierarchy traversal or scene interpretation;
- font discovery, shaping policy, or text-page rendering;
- blob reads, image decoding, or decoded-resource caches;
- renderer resource workers or retained-node construction;
- application persistence, history, or scene commits;
- PNG, PSD, or export policy.

`koharu-renderer` owns all persistent visual preparation. The application owns
the long-lived `Renderer`, awaits composition away from the desktop-state lock,
and installs completed results into the canvas.

## Composition lifecycle

The canvas accepts only a complete renderer `Composition`:

```rust,ignore
let generation = page_generation;
let composition = renderer.compose(&snapshot, page).await?;

if generation == current_page_generation {
    desktop.canvas.set_composition(composition)?;
}
```

After a scene commit, the application requests an incremental update and keeps
the old composition visible while work is in progress:

```rust,ignore
let next = renderer
    .update(&current_composition, &snapshot, &change)
    .await?;

if next.revision() == current_revision {
    desktop.canvas.set_composition(next)?;
}
```

The exact generation check belongs to the application because it owns page
navigation and the scene session. Installing a composition is synchronous and
cheap. Its immutable layers and retained nodes are reference-counted.

There is no `CanvasPage`, canvas resource hydration state, or duplicate element
materialization. Layer order, geometry, presentation, group ancestry, and O(1)
entity lookup come directly from `Composition`.

## Scene and pixel layers

The canvas has no hard-coded knowledge of page-source, cleanup, paint,
`rendered`, `text-mask`, or `coo-mask` roles. It draws the ordered color and mask
layers already present in the composition.

An explicit scene `PixelLayer` may describe either color content or a mask with
a channel and tint. This is sufficient for page artwork, imported images,
paint, cleanup, guides backed by pixels, and visible masks. Pipeline artifacts
that are not referenced by a visual layer are never loaded or displayed.

Mask editing is an interactive canvas concern, but mask persistence is not.
During a stroke, the canvas overlays sparse local mask tiles above the retained
composition. Finishing a stroke returns a commit payload. The application
stores the encoded blob and commits the corresponding explicit scene pixel
layer or asset update. A later renderer composition replaces the transient
overlay with retained content.

## Interactive path

Pointer and gesture updates must never await renderer work. The canvas applies
transient state immediately:

```text
pointer update
      |
      v
validate edit generation
      |
      v
update transform / sparse tiles / preview scene
      |
      v
mark exact damage
      |
      v
render next viewport frame
```

One `ActiveEdit` state machine represents the mutually exclusive native edit:

- transform preview;
- raster stroke preview;
- mask stroke preview.

Beginning a second edit while one is active is an error. Update sequence
numbers reject stale messages. Finishing returns a complete commit payload;
cancelling drops transient state without changing the scene.

The application and Web UI own tool choice, pointer interpretation, selection
policy, DOM handles, and semantic commits. The canvas owns native validation
and preview state for operations that affect its pixels.

## Sparse masks

Editable masks use fixed-size single-channel tiles allocated only when a stroke
touches them. Each tile tracks its nonzero count.

- Painting allocates only intersecting tiles.
- Erasing the final nonzero pixel releases the tile.
- A no-op stroke produces no generation or damage change.
- Dirty bounds contain only pixels whose values actually changed.
- Preview traversal visits occupied tiles rather than the full page grid.
- Tint conversion is cached and never performed with a per-frame full-page
  host loop.

Mask snapshots and commit encoding preserve sparsity until a serialized image
is explicitly required.

## Rendering and damage

The canvas builds a Vello scene from:

1. retained layers in the installed composition;
2. transient transform or stroke previews;
3. native mask overlays that have not yet been persisted.

DOM controls, selection handles, cursors, labels, and tool chrome remain in the
transparent WebView and do not invalidate the native canvas.

Damage is explicit. Recomposition occurs only when one of these changes:

- the installed composition;
- viewport size or camera transform;
- active preview content;
- local mask tiles;
- another native visual state.

Unchanged generations skip scene rebuilding, surface acquisition, and surface
submission.

## Viewport and GPU ownership

The native render target is exactly the physical viewport size, not the page
size and not the desktop-window size. The camera maps page coordinates into
local viewport coordinates. Content outside the viewport is clipped before
presentation.

The application owns the window surface and presents the canvas texture below
the transparent WebView. `koharu-canvas` owns only its offscreen target and the
Vello state needed to populate it.

Resize invalidates viewport-sized GPU resources and cancels incompatible
readbacks. A zero-sized viewport is valid and allocates no render target.

## Color sampling

Color sampling is asynchronous because WGPU mapping completes later, but the
canvas API must not block or hold the desktop lock while waiting.

The GPU layer owns a small fixed ring of reusable 256-byte readback buffers and
a bounded request queue. Request submission is synchronous; completion is
delivered through a callback or oneshot channel that an application command may
await.

The frame loop polls only while samples are outstanding and never calls a
blocking device wait. Resize, page clear, and drop cancel every user-visible
request deterministically.

Sampling observes the last successfully rendered viewport image.

## Async boundary

Canvas hot-path methods remain synchronous. Async orchestration stops before
the composition is installed:

- `Renderer::compose`, `Renderer::update`, and export rasterization are async.
- `Canvas::set_composition`, edit updates, camera changes, damage tracking, and
  scene construction are synchronous.
- GPU sample completion is asynchronous without making pointer or frame methods
  async.

An async method that performs no waiting is not added merely for API
uniformity. This keeps borrows short, avoids async locks around desktop state,
and prevents task scheduling overhead in pointer and frame loops.

## Performance invariants

- Never traverse a `Snapshot` in this crate.
- Never read or decode an asset in this crate.
- Never shape text or construct persistent renderer nodes in this crate.
- Clone compositions and layers through shared immutable storage.
- Keep transform placement outside retained local layer nodes.
- Append retained vector content directly; do not rasterize the page for
  interactive display.
- Allocate GPU targets from viewport dimensions only.
- Track exact mask and render damage.
- Bound sample queues and reuse readback allocations.
- Do no native work for DOM-only changes.

## Extension rules

- New persistent visual behavior belongs in scene components and the renderer.
- New transient native editing behavior may belong here if it owns actual
  viewport state or pixels.
- Do not introduce another page, layer, resource, or typography model.
- Do not add asset-role-specific display modes.
- Do not add canvas-owned decode workers or renderer caches.
- Do not hold application or desktop locks across renderer awaits.
- React/WebView and native WGPU responsibilities must remain visually composable
  in the final desktop window.

## Validation

During development, run the smallest focused debug check or test covering the
changed path. Typical crate-level checks are:

```text
cargo check -p koharu-canvas
cargo test -p koharu-canvas <focused-test-name>
```

Real-GPU visual checks remain explicit because they require a usable adapter and
must validate the final native-under-WebView composition.
