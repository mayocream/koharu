# koharu-renderer

This document defines the target architecture for `koharu-renderer`. During the
migration, implementation details may temporarily differ from this contract.
Replaced APIs must be removed rather than preserved through compatibility
layers.

`koharu-renderer` is the single owner of turning an authored
`koharu-scene::Snapshot` into reusable visual content. The editor canvas, image
export, previews, and PSD export all consume the same `Composition`.

```text
Snapshot + page
       |
       v
Renderer::compose / Renderer::update
       |
       v
Composition
       |---------------------> Canvas vector embedding
       |---------------------> PSD layer projection
       `---- Renderer::rasterize ----> CPU-readable pixels
```

There is one renderer service and one renderer-side page representation. The
crate does not expose compilation stages, hydration stages, or alternate frame
types.

## Ownership

`Renderer` owns resources and expensive reusable state:

- font discovery, loading, fallback resolution, and shaping caches;
- blob reads and a bounded image/mask decode queue;
- a byte-bounded decoded-resource cache;
- retained per-layer Vello nodes;
- text layout and glyph recording;
- headless GPU state, reusable raster targets, and readback buffers;
- incremental reuse statistics and render diagnostics.

The application creates one long-lived `Renderer` and clones its shared handle
where necessary. Creating a renderer per page, export, or preview is incorrect
because it discards the caches that make rendering inexpensive.

The renderer does not own:

- persistent entities, hierarchy, components, relations, or assets;
- tool state, selection, camera state, or interactive previews;
- PSD structure, PNG encoding policy, or application persistence;
- semantic guesses about which asset or text should be displayed.

Those responsibilities remain in `koharu-scene`, `koharu-canvas`, and their
format-specific consumers.

## Scene contract

Renderable intent must be explicit in the scene. A generic pixel component can
reference any stored image or mask:

```rust,ignore
pub struct AssetRef {
    pub owner: EntityId,
    pub role: AssetRole,
}

pub struct PixelLayer {
    pub origin: Origin,
    pub name: String,
    pub asset: AssetRef,
    pub format: PixelFormat,
}

pub enum PixelFormat {
    Color,
    Mask {
        channel: MaskChannel,
        tint: [u8; 4],
    },
}
```

`Geometry` owns placement. `Visibility` owns visibility and opacity. Hierarchy
owns paint order and group ancestry. Text layout and typography components own
authored text presentation. Defaults are materialized by the editor or import
pipeline before committing the scene; they are not supplied through a render
request.

The original page, cleanup artwork, paint layers, imported images, and masks
are all ordinary explicit pixel layers. The renderer has no built-in knowledge
of asset roles such as `rendered`, `text-mask`, or `coo-mask`. Pipeline
artifacts remain non-visual unless a scene entity explicitly references them
through `PixelLayer`.

Analysis entities are not visual merely because they contain geometry or an
asset. They become visible only through an explicit visual component.

## Public model

`Composition` is an immutable ordered collection of renderable layers for one
page revision:

```rust,ignore
pub struct Composition {
    revision: Revision,
    page: EntityId,
    size: Size,
    layers: Arc<[Layer]>,
    index: EntityIndex,
    diagnostics: Arc<[Diagnostic]>,
}

pub struct Layer {
    pub entity: EntityId,
    pub geometry: Geometry,
    pub presentation: Presentation,
    pub ancestry: Arc<[EntityId]>,
    pub kind: LayerKind,
    retained: Arc<RetainedNode>,
}

pub enum LayerKind {
    Pixel(PixelMetadata),
    Text(TextMetadata),
}
```

The retained node is private renderer machinery. Consumers can append a whole
composition or one selected layer to a caller-owned Vello scene without
accessing its cache representation.

`Composition` provides ordered iteration and O(1) entity lookup. It is cheap to
clone because immutable layer storage and retained nodes are reference-counted.
It is the only value shared by canvas, raster output, and PSD projection.

There is no `VisualPage`, `CompiledPage`, `PreparedPage`, `Frame`,
`PreparedEntity`, or canvas-specific copy of the page.

## Public operations

Composition and pixel production are asynchronous because they may wait for
storage, font data, decoding, GPU execution, or readback:

```rust,ignore
impl Renderer {
    pub async fn compose(
        &self,
        snapshot: &Snapshot,
        page: EntityId,
    ) -> Result<Composition>;

    pub async fn update(
        &self,
        previous: &Composition,
        snapshot: &Snapshot,
        change: &Change,
    ) -> Result<Composition>;

    pub async fn rasterize(
        &self,
        composition: &Composition,
        options: &RasterOptions,
    ) -> Result<RasterImage>;
}
```

A successful `compose` or `update` returns a complete, renderable composition.
There is no second `compose_complete` operation, public pending-resource state,
or hydration API. Interactive callers keep displaying their previous
composition until the new future completes.

Vector embedding and read-only access remain synchronous:

```rust,ignore
composition.append_to(&mut scene, view_transform);

if let Some(layer) = composition.layer(entity) {
    layer.append_to(&mut scene, preview_transform);
}
```

`RasterOptions` contains only final pixel-output choices such as scale,
background, and color format. Page selection, asset selection, text fallback,
and typography are not raster options.

## No render request

The crate has no `RenderRequest` and no `request` module.

- The page is a direct `compose` argument.
- Explicit scene layers replace base-asset and image-role selection.
- A composition always includes the complete authored page.
- Consumers select existing layers for cropped or partial output without
  recompiling the page.
- Visibility and opacity remain layer presentation data.
- The scene's explicit content relation determines displayed text.
- Typography and layout are authored scene data.

This gives every successful composition one deterministic meaning: it is the
visual page described by that snapshot.

## Async and scheduling

The crate follows one rule: asynchronous orchestration, synchronous
computation.

A composition operation performs coarse-grained work:

1. Traverse the page once and build small layer descriptors synchronously.
2. Deduplicate all required fonts and blobs.
3. Read missing resources asynchronously in bounded batches.
4. Decode images and shape text on a bounded CPU pool.
5. Reuse or build retained Vello nodes synchronously.
6. Return one immutable `Composition`.

The renderer must not create one future per entity, glyph, or tile. Blocking
storage and decode work must never run on the async executor. Cache access must
not introduce async locks into scene traversal or Vello recording.

Dropping an obsolete composition future prevents its result from being
installed. Already completed immutable resource decoding may remain in the
shared cache for later requests.

## Incremental reuse

Incremental rendering is deliberately simple. For a relevant active-page
change, `update` traverses that page once and rebuilds lightweight descriptors.
Layers are matched by entity ID and exact descriptor equality.

- Unchanged descriptors reuse the same retained node.
- Translation, rotation, visibility, and opacity do not rebuild local content.
- Text rebuilds only when content, typography, frame constraints, or the
  resolved font generation changes.
- Pixel content rebuilds only when its blob or pixel interpretation changes.
- Removed layers are dropped; inserted layers are built once.

An O(n) descriptor walk is preferred to a public dependency-address graph. The
expensive work is blob access, decoding, shaping, and GPU allocation, all of
which remains retained. Dependency tracking may exist privately where a
measurement proves it useful, but it must not create another public page model.

## Rasterization

Rasterization consumes the same `Composition` used by the canvas. It does not
re-read the scene or reinterpret layers. Cropped entity output is derived from
an existing layer before allocating the GPU target, so PSD export does not
rasterize a full page for every layer.

The renderer retains its Vello device state, glyph caches, readback buffers,
and a small target pool across calls. GPU completion and mapping are awaited
without blocking the application event loop.

## Performance invariants

- Traverse a page at most once per composition attempt.
- Batch and deduplicate resource requests before scheduling them.
- Bound storage, decode, and shaping concurrency.
- Never decode images or discover fonts in the canvas or an export consumer.
- Keep placement and presentation outside retained local layer nodes.
- Avoid full-page intermediate rasters during interactive rendering.
- Reuse immutable nodes and decoded assets across canvas, previews, PSD, and
  image export.
- Measure before adding finer-grained caches or dependency machinery.

## Extension rules

- New authored visual concepts begin as explicit scene components.
- Scene interpretation and retained visual construction belong here.
- Canvas-specific state must not enter `Composition`.
- Format-specific metadata and serialization remain outside this crate.
- Do not add alternate composition types for individual consumers.
- Do not add policy bags that allow callers to reinterpret the same snapshot
  differently.
- An async function must own real waiting or scheduling; pure helpers remain
  synchronous.

## Validation

During development, run the smallest focused debug check or test covering the
changed path. Typical crate-level checks are:

```text
cargo check -p koharu-renderer
cargo test -p koharu-renderer <focused-test-name>
```

GPU visual tests remain explicit because they require a usable adapter.
