# koharu-renderer

`koharu-renderer` turns one page from `koharu-scene` into retained Vello content
and, when requested, pixels. The editor canvas, flattened image export, and PSD
export must all use the same rendering result.

This document describes a behavior-preserving redesign with explicit dead-code
removal. Rust APIs may change, but scene meaning, active product behavior,
editor behavior, export behavior, and document compatibility may not change as
a side effect of the refactor. Dormant policy with no intended product use is
deleted rather than carried into the new design.

## Design constraints

- Keep the existing crate. Do not introduce another rendering crate.
- Use the names `Renderer`, `Frame`, `Layer`, and `Raster`; do not introduce
  `RenderEngine`, `RenderPage`, or similarly indirect names.
- A caller asks for a rendered page once. There is no public `compose` /
  `compose_complete` pair and no public intermediate `Composition`.
- Remove `request.rs` and `RenderRequest`. The complete rendering input is a
  `Snapshot` and page ID; there is no renderer configuration object.
- Do not add compatibility wrappers or aliases. Update in-repository callers
  directly when the implementation is migrated.
- Every setting, diagnostic, cache, and public method must have a current
  in-repository consumer. Remove policy that exists only for hypothetical use.
- Operations that can perform storage I/O, font loading, image decoding, or GPU
  work are asynchronous. Access to an already built `Frame` stays synchronous.
- Refactoring ownership is not permission to change product behavior.

## Preserved behavior

The new implementation must first reproduce the active renderer behavior:

| Area | Required behavior |
| --- | --- |
| Page background | Render the document's source page image. There is no caller-selected asset-role preference list. |
| Child images | Render explicit image/raster entities in scene order with their geometry. |
| Text choice | Render `Translation` only. `SourceText` remains semantic OCR input and editable document data, but is never visual renderer input. A text entity without a translation produces no glyph content. |
| Typography | Preserve writing mode, alignment, font chain, fallback, size, auto-fit, line height, spacing, insets, fill, and stroke. |
| Fitting | Preserve text-to-region and text-to-bubble fitting, including overflow diagnostics without clipping authored glyphs. |
| Presentation | Preserve group ancestry, visibility, opacity, resolved presentation, and deferred presentation used by interactive previews. |
| Layer access | Preserve ordered layers, per-entity lookup, per-entity vector embedding, and tightly cropped entity output. |
| Diagnostics | Preserve missing assets, overflow, minimum readable size, and resource errors as applicable. |
| Export | PNG and PSD must consume the same retained vector result as the canvas. |
| Limits | Preserve finite/positive dimension checks, surface limits, decode validation, and supersampling bounds. |

Source-text fallback is intentionally removed. Delete
`fallback_to_source_text`, `RenderDiagnostic::UsedSourceText`, their branches,
and their tests. Output, previews, PSD export, pipeline utilities, and canvas
must all use the same translation-only rule. No caller-specific fallback flag
replaces it.

Any other intentional product change must be implemented and tested separately
from the ownership migration.

## Ownership

### `koharu-scene`

The scene owns persistent meaning:

- page size and hierarchy;
- asset roles and blob references;
- text content, translations, typography, geometry, visibility, and relations;
- revision and change information.

The renderer does not create a second document model and does not write to the
scene.

### `Renderer`

One long-lived `Renderer` owns all reusable rendering resources:

- font discovery, bundled-font loading, and font caches;
- decoded-image cache and bounded decode workers;
- retained vector nodes and their dependency index;
- the headless Vello rasterizer, GPU context, readback buffers, and target pool.

Cloning `Renderer` shares this state. Construction is cheap and performs no
blocking work.

### `Frame`

`Frame` is the only public rendered-page value. It is not a forwarding wrapper;
it owns the immutable result needed by every consumer:

- page ID, scene revision, size, and page-space origin;
- ordered retained layers;
- O(1) entity lookup;
- per-layer geometry, presentation, ancestry, and text metadata;
- dependencies, diagnostics, and retention statistics;
- whole-page and per-layer Vello scenes.

`Frame` contains no `Snapshot`, mutable cache, caller policy, or storage handle.
Cloning it is O(1).

### `Raster`

`Raster` owns CPU-readable RGBA pixels plus their page-space origin. A full
page and a cropped layer use the same type.

## Public API shape

The target API has one rendering entry point:

```rust,ignore
let renderer = Renderer::new()?;

let frame = renderer
    .render(&snapshot, page)
    .await?;

let updated = renderer
    .update(&frame, &snapshot, &change)
    .await?;

let raster = renderer
    .rasterize(&updated, RasterOptions::default())
    .await?;
```

`render` is complete when it returns. It performs scene interpretation,
resource loading, and retained-node construction internally. A caller never
coordinates a compositor and a scene renderer and never supplies a request
object.

There is no settings parameter. Asset selection, layer inclusion, text choice,
typography, and presentation all come from the scene. Renderer-owned safety
limits and cache budgets are implementation invariants, not per-render policy.

`update` produces the same result as `render` for the new snapshot. It is only
an optimization: unchanged retained nodes are reused through the change and
dependency indexes. Incorrect or non-contiguous revision input is rejected.

Scene data determines what is rendered. `Frame` retains authored presentation
metadata so canvas can override visibility and opacity transiently without a
second document-rendering path. `Frame::cropped` provides one-layer output
without an entity-selection input to `render`. Raster options remain separate
because they affect pixel production, not vector content.

Already rendered data is synchronous:

```rust,ignore
frame.append_to(&mut scene, None);

if let Some(layer) = frame.layer(entity) {
    layer.append_to(&mut scene, Some(preview_transform));
}

if let Some(cropped) = frame.cropped(entity)? {
    let raster = renderer.rasterize(&cropped, options).await?;
}
```

Font-family discovery and preview generation are asynchronous methods on
`Renderer`; callers do not own a separate public font service.

## Internal rendering path

One call to `render` has four internal phases. These are functions and private
data, not public lifecycle objects.

```text
Snapshot + page
          |
          v
single scene traversal -> private layer descriptors + dependencies
          |
          v
deduplicated resource plan -> batched async blob/font loading
          |
          v
parallel retained-node construction or cache reuse
          |
          v
immutable Frame
```

The scene is traversed once. Cross-entity text, fit, asset, and presentation
relationships are resolved during that traversal. Resource IDs are
deduplicated before any I/O. Image decoding runs on a bounded renderer-owned
pool; it never uses the global Rayon pool and never blocks the async caller.

Text shaping and glyph recording remain specialized internal modules because
they own real algorithms. They are not public pipeline stages.

## Retention and invalidation

Each retained node is keyed by the scene values and resources that affect its
visual output. A `Frame` records exact entity, hierarchy, component, relation,
blob, and font dependencies.

`update` uses `Change` to identify candidates, then compares descriptors:

- unchanged descriptors reuse the same `Arc` node;
- placement-only changes reuse vector content and replace placement metadata;
- changed visual descriptors rebuild only their node;
- insertion of a previously absent relation is detected through relation-query
  dependencies;
- immutable blobs are cached by `BlobId`.

Cache capacity is bounded. Eviction affects performance only, never output.

## Concurrency

- Snapshot interpretation is deterministic and read-only.
- Blob reads, image decoding, font loading, and raster readback occur away from
  the caller's async executor.
- Resource batches are deduplicated before scheduling.
- Independent changed layers may be built in parallel on bounded workers.
- Vello GPU submission is serialized only around the state Vello actually
  mutates.
- No method holds a renderer cache lock while awaiting work.

Making a function `async` does not make CPU work non-blocking; blocking work
must be moved to the renderer's bounded workers.

## Module boundaries

The target source layout reflects owners rather than public stages:

| Module | Responsibility |
| --- | --- |
| `renderer` | `Renderer`, scene traversal, resource planning, and update orchestration. |
| `frame` | Immutable `Frame`, `Layer`, metadata, indexes, vector embedding, cropping. |
| `fonts` | Discovery, bundled/system loading, fallback, and font cache. |
| `text` plus layout modules | Unicode segmentation, shaping, fitting, layout, and glyph recording. |
| `images` | Batched blob loading, decode validation, and byte-bounded cache. |
| `raster` | Vello execution, reusable targets, readback, and downsampling. |
| `error` | Errors that retain their underlying causes. |

There is no `request`, `compositor`, or `scene_renderer` public subsystem.
Private helpers are kept only when they own a substantial algorithm or state.

## Migration and verification

The migration must be behavior-first:

1. Add characterization tests around the active renderer behavior before
   replacing it.
2. Record representative page output, ordered layer metadata, diagnostics,
   entity crops, translation-only rendering, and source-only invisibility.
3. Implement `Renderer` and `Frame` behind the new API.
4. Update canvas, PNG output, PSD output, previews, and pipeline callers
   directly; add no compatibility layer.
5. Compare old and new output on identical snapshots. Pixel differences must
   be zero unless a separately approved rendering fix explains them.
6. Delete the replaced modules only after every in-repository consumer and
   characterization test passes.
7. Audit the completed API for inputs and diagnostics with no product consumer;
   delete them instead of documenting hypothetical use.

Focused validation:

```text
cargo test -p koharu-renderer
cargo check -p koharu-renderer --all-targets
```

GPU tests remain explicitly opt-in on a machine with a usable Vello adapter.
