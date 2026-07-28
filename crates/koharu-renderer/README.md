# koharu-renderer

This document describes the current design and implementation contract of
`koharu-renderer`.

## Purpose

`koharu-renderer` turns one immutable `koharu-scene` page revision into either:

- a prepared Vello scene that can be embedded in another renderer; or
- an RGBA image produced by the reusable headless WGPU backend.

The crate owns rendering policy and transient rendering state. It interprets
semantic scene components, resolves fonts, lays out multilingual text, decodes
images, records Vello commands, caches reusable work, and performs GPU
rasterization.

It does not persist visual implementation details back into `koharu-scene`.
Glyph IDs, line breaks, decoded pixels, fitted font sizes, Vello scenes, GPU
textures, and renderer caches are all derived state.

## Decisions

| Concern | Current decision |
| --- | --- |
| Input | An immutable `koharu_scene::SceneSnapshot` plus a complete `RenderRequest`. |
| Architecture | Compile semantic intent, prepare resources and draw commands, then rasterize. |
| Scene coupling | Consume typed scene capabilities instead of a renderer-specific element enum. |
| Visual policy | Keep non-persistent defaults in `RenderTheme`; scene typography remains user intent. |
| Text | HarfRust shaping, Unicode BiDi, ICU4X line breaking and script data, Jieba Chinese segmentation, and Hypher hyphenation. |
| Font discovery | Fontique system and registered fonts with language- and script-aware fallback. |
| Symbols | A renderer-owned ordered symbol and emoji fallback policy. |
| Vertical text | Native top-to-bottom shaping with `vert`/`vrt2`, right-to-left or left-to-right column flow, and CJK punctuation adjustment. |
| Parallelism | Prepare independent page layers in parallel while preserving deterministic scene order. |
| GPU | One reusable Vello/WGPU backend with persistent glyph caches and a small render-target pool. |
| Reuse | Bounded decoded-image, resolved-font, prepared-page, and render-target caches. |
| Invalidation | Track scene dependencies and consume `SceneChangeSet` rather than clearing every prepared page. |
| Output | `image::RgbaImage` plus revision, entity, dependency, and diagnostic metadata. |

## Boundary with koharu-scene

```text
koharu-scene
    semantic components, hierarchy, relations, revisions, blobs
             |
             v
RenderPlan::compile
    resolve page intent into owned backend-independent layers
             |
             v
PreparedPage::prepare
    decode images, resolve fonts, lay out text, record Vello scenes
             |
             +---------------------> PreparedPage::append_to
             |                       interactive/caller-owned Vello scene
             v
WgpuRenderer::rasterize
    GPU render, readback, optional downsample
             |
             v
RenderOutput
    RGBA pixels and traceable render metadata
```

Scene owns meaning. Renderer owns presentation. In particular:

- `Geometry` states where an entity exists; the renderer derives a temporary
  layout rectangle from its point bounds.
- `SourceText` and locale-keyed `Translation` store text; the renderer derives
  script runs, glyphs, lines, and fitted sizes.
- `Typography` stores durable intent such as preferred font, requested size,
  alignment, and writing mode; `RenderTheme` supplies export or application
  policy without modifying the scene.
- `Asset` describes a blob and its media metadata; renderer resources own
  decoded pixels.
- `Region` and explicit relations express semantic layout relationships;
  bubble padding and final placement are renderer policy.

The renderer never guesses a text-to-bubble association from spatial overlap.
It uses a relation from the text entity to a matching region entity. The
default identifiers are:

```text
dev.koharu.relation.text-region
dev.koharu.region.bubble
```

Both identifiers can be replaced on `RenderRequest`.

## Public entry points

### High-level page rendering

`Renderer` is the normal application entry point:

```rust,no_run
use koharu_renderer::{RenderRequest, Renderer};
use koharu_scene::{EntityId, SceneSnapshot};

fn render_page(snapshot: &SceneSnapshot, page: EntityId) -> koharu_renderer::Result<()> {
    let renderer = Renderer::new()?;
    let request = RenderRequest::new(page);
    let output = renderer.render(snapshot, &request)?;

    output.image.save("page.png").unwrap();
    Ok(())
}
```

`Renderer::new` initializes the WGPU backend immediately. Code that only needs
semantic planning can call `RenderPlan::compile` directly without requiring a
GPU.

The facade exposes the stages separately when the caller needs inspection or
interactive composition:

```rust,no_run
# use koharu_renderer::{RenderRequest, Renderer};
# use koharu_scene::{EntityId, SceneSnapshot};
fn prepare_page(
    renderer: &Renderer,
    snapshot: &SceneSnapshot,
    page: EntityId,
) -> koharu_renderer::Result<vello::Scene> {
    let request = RenderRequest::transparent(page);
    let plan = renderer.compile(snapshot, &request)?;
    assert_eq!(plan.revision(), snapshot.revision());

    let prepared = renderer.prepare(snapshot, &request)?;
    let mut scene = vello::Scene::new();
    prepared.append_to(&mut scene, None);
    Ok(scene)
}
```

### Low-level text rendering

`FontSystem`, `TextLayout`, and `WgpuRenderer` remain public for callers that
need to render text without compiling a scene page:

```rust,no_run
use koharu_renderer::{
    FontSystem, RenderOptions, TextLayout, WgpuRenderer, WritingMode,
};

fn render_text() -> anyhow::Result<image::RgbaImage> {
    let mut fonts = FontSystem::new();
    let font = fonts.first_font()?;
    let layout = TextLayout::new(&font)
        .with_font_size(24.0)
        .with_max_width(640.0)
        .run("Hello, 世界!!")?;

    WgpuRenderer::new()?.render(
        &layout,
        WritingMode::Horizontal,
        &RenderOptions::default(),
    )
}
```

This path deliberately does not use `RenderResources` or the prepared-page
cache. The caller owns the resolved fonts and layout lifetime.

## RenderRequest and RenderTheme

`RenderRequest` is a complete, cloneable description of one render. It selects:

- the page and optional output locale;
- ordered page base-asset roles;
- the asset role used for child image entities;
- whether child images are included;
- whether a missing translation falls back to source text;
- the text-region relation and bubble region kinds;
- non-persistent visual theme values; and
- raster quality options.

`RenderRequest::new(page)` prefers page assets in `clean`, then `source`, order.
`RenderRequest::transparent(page)` removes the base image and produces a
transparent page unless other layers cover it.

`RenderTheme` currently controls ordered text font families, maximum and
minimum font size, fill and optional stroke, line height, letter and word
spacing, text inset, vertical placement, and whether text auto-fits its bounds.
Insets are stored in top, right, bottom, left order.

Theme values are validated before scene traversal. Invalid or non-finite
dimensions fail the render rather than entering text or GPU code.

Raster options are intentionally excluded from the prepared-page cache key.
The same prepared Vello scene can therefore be exported repeatedly at different
supersampling settings without reshaping or decoding it.

## Stage 1: semantic compilation

`RenderPlan::compile` is a read-only pass over one `SceneSnapshot`. Its output
owns all layer data needed by preparation and is tied to the snapshot revision.
It performs no image decoding and creates no GPU objects.

Compilation proceeds in deterministic scene order:

1. Validate the request and page surface.
2. Select the first available page base asset from `base_assets`.
3. Traverse the page subtree in hierarchy order.
4. Ignore invisible entities, zero-opacity entities, and entities without
   usable geometry.
5. Add a child image layer when the requested asset role is present.
6. Resolve translated or source text.
7. Resolve writing mode, text direction, alignment, preferred font, and size.
8. Retain the related bubble's raw bounds and contour when an explicit matching
   relation exists; preparation derives temporary text-flow policy from them.
9. Record every entity, relation, and blob on which the plan depends.

Layer order is stable: the base image comes first, then each entity contributes
its image and text layers while walking the subtree.

### Locale resolution

When `request.locale` is set, a `Translation` in the locale slot is preferred.
If it is missing and `fallback_to_source_text` is true, `SourceText` is used and
`RenderDiagnostic::UsedSourceText` is emitted. If fallback is disabled, that
text layer is omitted. Without a requested locale, source text is used.

The resolved `LanguageTag` is retained in the layer and later passed into font
fallback and hyphenation selection.

### Writing mode and alignment

An explicit `Typography.writing_mode` wins. Otherwise non-CJK text is
horizontal. CJK text uses `OcrAnalysis.direction` when it is explicit, falling
back to vertical for a taller-than-wide box and horizontal otherwise.

Vertical scene text currently maps to `WritingMode::VerticalRl`. The low-level
layout API additionally supports `VerticalLr`.

Logical start/end alignment is resolved against the paragraph direction.
Defaults are centered.

### Bubble layout

The renderer accepts the first valid outgoing text-region relation whose target:

- is inside the same page subtree;
- has a `Region` with the requested kind; and
- has usable `Geometry`.

The plan retains the bubble's axis-aligned bounds and its finite local contour.
Preparation derives horizontal per-line width targets by intersecting an ideal
ellipse with the widest interior span of the actual contour at each line. The
line block is vertically centered, explicit newlines participate in the same
global profile, and asymmetric contours shift each line toward the available
interior instead of being recentered in the bounding box. Contours above 1,024
points fall back to the ellipse profile to bound preparation cost.

Bubble air is font-relative rather than a percentage of the region: it is the
larger of the shaped advance of `o` and `0.5em`. The air is removed from the
ellipse and contour spans and from the usable vertical extent. Vertical text
uses the same font-relative outer air but does not yet flow columns through the
contour. `RenderTheme::text_inset` remains an optional additional fixed inset.

### Dependencies and diagnostics

`RenderPlan` and `PreparedPage` expose sorted `RenderDependency` values for
entities, relations, and blobs. These values support cache invalidation and
make the result traceable.

Recoverable policy outcomes are diagnostics: a missing requested base asset,
source-text fallback, text that still overflows its available bubble or box,
and an explicit font size below the configured readable floor. Invalid data and
backend failures remain errors.

## Stage 2: parallel preparation

`PreparedPage::prepare` transforms plan layers independently with Rayon and
collects them in the plan's original order. Parallel execution therefore does
not change compositing order.

Each image layer:

- fetches the blob through `SceneSnapshot`;
- decodes it to RGBA8 through `image`;
- validates dimensions when both width and height are declared in metadata;
- requires a base image to exactly match the page surface;
- records a scaled Vello image with normal alpha compositing; and
- reuses decoded bytes through the bounded image cache.

Each text layer:

- applies theme insets to its semantic bounds;
- resolves an ordered font chain;
- lays out and optionally auto-fits text;
- places the tight text result horizontally centered and vertically according
  to `VerticalAlignment`;
- clips drawing to the inset layout box;
- records optional stroke before fill; and
- reports the final bounds and fitted font size as `RenderedEntity` metadata.

The resulting `PreparedPage` owns an immutable `Arc<vello::Scene>`. It is safe
to reuse for raster exports and to append to caller-owned Vello scenes without
GPU readback.

## Text system

### Shaping and fallback

HarfRust performs OpenType shaping. ICU script properties divide text into
script runs, and Unicode BiDi levels determine horizontal visual order and
per-run direction. Fallback splitting preserves extended grapheme clusters, so
combining marks, variation selectors, and emoji ZWJ sequences are not split
between fonts scalar by scalar.

Every `PositionedGlyph` retains the exact resolved `Font` used to shape and
draw it. Synthetic weight and skew requested by Fontique are carried through
metrics and Vello drawing.

### Line breaking

ICU4X supplies Unicode line-break opportunities. Optional Jieba segmentation
keeps Chinese word boundaries together without replacing ICU punctuation
rules. Hypher adds discretionary hyphens for supported language tags and long
words. A discretionary hyphen is shaped only when its candidate is selected.

Uniform-box line selection minimizes cubic raggedness with explicit overflow
and hyphenation penalties. Horizontal balloon text instead evaluates feasible
line counts against its ellipse/contour width profile. Its cost prefers breaks
after sentence and clause punctuation, then before common conjunctions, and
discourages breaks after short articles and prepositions. Shape fit wins over
stylistic cost, so punctuation preferences cannot hide overflow. Mandatory
newlines constrain the global solution but are never shaped as visible glyphs.

For a horizontal balloon whose resolved `LanguageTag` has primary language
`en`, discretionary hyphenation is last-resort: the optimizer first searches
all unhyphenated layouts and only admits hyphenated breaks when every such
layout overflows. Other low-level layout uses retain normal hyphenation unless
the caller selects `HyphenationPolicy::LastResort` or `Disabled`.

### Auto-fit and metrics

Fixed-size layout uses `with_font_size`. Auto-fit searches between maximum and
minimum readable sizes. If the minimum size still overflows, preparation may
tighten line height down to its configured floor before accepting overflow.
The accepted result exposes `LayoutRun::overflowed`; prepared scene text emits
`RenderDiagnostic::TextOverflow` with available and actual dimensions. An
explicit scene size below `RenderTheme::minimum_font_size` emits
`RenderDiagnostic::TextBelowReadableSize` instead of silently treating it as a
normal readable size.

Font metrics determine ascent, descent, leading, baselines, and line spacing.
Final layout dimensions use per-glyph ink bounds with a small antialiasing
safety pad rather than only logical advances. Synthetic skew and emboldening
are included in those bounds.

Public line ranges and glyph clusters are UTF-8 byte offsets into the caller's
original text.

### Emphasis punctuation

Emphasis-pair normalization applies to both horizontal and vertical text before
line breaking and shaping:

| Input pair | Shaped symbol |
| --- | --- |
| `!!` or `！！` | `‼` |
| `??` or `？？` | `⁇` |
| `!?` or `！？` | `⁉` |
| `?!` or `？！` | `⁈` |

Mixed ASCII and fullwidth pairs are accepted. Runs of three preserve the
strongest adjacent pair: for example, `?!!` becomes `?‼`, `!!?` becomes `‼?`,
and `!?!` becomes `⁉!`.

Normalization maintains an offset map and remaps layout ranges and glyph
clusters back to the original source. Text without a pair remains borrowed and
does not allocate a normalized copy.

### Vertical CJK text

Vertical layout uses top-to-bottom HarfRust direction and enables the OpenType
`vert` and `vrt2` features. Columns flow right-to-left for `VerticalRl` and
left-to-right for `VerticalLr`.

Fullwidth CJK punctuation and the combined emphasis symbols are optically
centered from their real glyph bounds. This adjustment is enabled by default
and can be disabled with `with_center_vertical_punctuation(false)`. It is a
vertical-only adjustment; emphasis-pair normalization itself applies in every
writing mode.

## Font resources and policy

`RenderResources` owns a thread-safe `FontManager` and decoded-image cache.
Resources can be shared by multiple renderer instances through `Arc`.

`FontManager` wraps one Fontique collection and source cache. It supports:

- system font discovery;
- stable-key registration of caller-provided font bytes;
- face enumeration;
- PostScript-name resolution for ergonomic UI access; and
- cached resolution by family, attributes, scripts, and language.

Registering new font bytes increments a generation counter. That counter is
part of the prepared-page cache key, so pages prepared with an older font set
cannot be reused accidentally.

The renderer's font order is:

1. `Typography.preferred_font`, when present;
2. ordered `RenderTheme.font_families`;
3. Fontique's script- and language-appropriate platform fallback; and
4. the explicit symbol/emoji safety-net families.

The default `FontFallbackPolicy` restores this ordered safety net:

```text
Segoe UI Symbol
Segoe UI Emoji
Noto Sans Symbols
Noto Sans Symbols2
Noto Color Emoji
Apple Color Emoji
Apple Symbols
Symbola
Arial Unicode MS
```

Unavailable family names are ignored. Callers can replace the list with
`FontFallbackPolicy::new` or disable it with
`FontFallbackPolicy::disabled`. Symbol families are queried separately as a
final fallback so a missing preferred font does not turn a symbol face into the
body font.

## Stage 3: Vello and WGPU rasterization

`WgpuRenderer` initializes WGPU and Vello once. It keeps Vello's internal glyph
state and up to four reusable render targets alive across calls.

The backend renders to `Rgba8Unorm` with Vello area antialiasing, copies the
texture to an aligned readback buffer, removes row padding, and returns an
`image::RgbaImage`.

GPU command recording is serialized because Vello mutates renderer caches.
The lock is released after queue submission; device polling and buffer readback
then happen without holding the renderer lock. Independent callers can overlap
GPU completion even though command encoding is ordered.

`RasterOptions` supports 1x through 4x supersampling. Values outside that range
are clamped. Supersampled output is reduced with `fast_image_resize` using
nearest, triangle, Catmull-Rom, Gaussian, or Lanczos3 filtering. The default is
1x because Vello already provides area antialiasing; Lanczos3 is the default
filter when supersampling is requested.

The page facade always rasterizes onto transparent black. The low-level
`WgpuRenderer::render` API additionally supports an explicit background,
padding, baseline shift, hinting policy, fill, and stroke.

There is currently no CPU raster fallback.

## Caches and invalidation

The crate uses bounded caches at distinct ownership levels:

| Cache | Owner | Current bound | Invalidation |
| --- | --- | --- | --- |
| Resolved fonts | `FontSystem` | 256 resolution keys before clearing | Font registration clears resolutions. |
| Decoded RGBA images | `RenderResources` | 512 MiB by default | Byte-bounded LRU; explicit `clear_images`. |
| Prepared pages | `Renderer` | 8 pages | LRU; request, revision, and font generation key; scene change sets. |
| GPU targets | `WgpuRenderer` | 4 targets | Reused by exact raster size for the backend lifetime. |

The prepared key includes the entire render request except raster options. A
font registration produces a different key through the font generation.

`Renderer::apply_changes` accepts the `SceneChangeSet` from a successful scene
commit. It advances unaffected prepared pages to the new revision without
rebuilding their Vello scene and drops entries whose recorded dependencies
changed. Page-list changes, relation changes, and entity insertion currently
take the conservative path because they can affect traversal or semantic
resolution globally. `clear_cache` remains available for explicit reset.

The prepared cache is page-granular. Independent layers are prepared in
parallel, but a changed dependency currently causes the affected prepared page
to be rebuilt rather than reusing unchanged layer scenes individually.

## Concurrency model

- `SceneSnapshot` is immutable and can be read concurrently.
- `RenderPlan` owns its resolved values and has no GPU state.
- Layer preparation uses Rayon and preserves input ordering when collecting.
- `FontManager` serializes access to Fontique's mutable caches.
- Decoded image cache access is synchronized; decoding itself occurs outside
  the cache lock.
- Prepared-cache operations use a short mutex.
- Vello command recording is serialized, while GPU completion and readback can
  overlap between callers.

The public API is synchronous. Concurrency belongs to the caller and the
internal Rayon/WGPU implementation; the crate does not depend on an async
runtime.

## Validation and limits

The renderer rejects invalid dimensions before allocating large buffers.
Current page-plan limits are:

- maximum width or height: 32,768 pixels;
- maximum page area: 268,435,456 pixels; and
- non-zero finite theme dimensions and spacing; and
- at most 1,024 contour points used for profiled balloon flow and 64 evaluated
  lines per balloon layout solution.

The raster backend additionally checks supersampling overflow and the selected
WGPU device's maximum texture dimension. Image metadata is checked against the
decoded image when width and height were declared. A base image must exactly
match the page size.

Errors retain their boundary:

- scene access and validation errors;
- invalid render requests;
- per-blob image decode errors;
- per-entity font and text-layout errors;
- font resource operation errors; and
- backend initialization, rendering, polling, or readback errors.

## Module map

| Module | Responsibility |
| --- | --- |
| `request` | Complete render request and transient theme policy. |
| `plan` | Scene interpretation, layer ordering, dependencies, diagnostics, and request validation. |
| `bubble` | Explicit text-region resolution, raw bounds, and bounded local contours. |
| `prepare` | Parallel image/text preparation and immutable Vello page recording. |
| `resources` | Shared font manager and bounded decoded-image cache. |
| `font_policy` | Configurable renderer-owned symbol and emoji families. |
| `font` | Fontique discovery, registration, resolution, and resolved font instances. |
| `script` | Unicode script detection, CJK detection, and shaping direction. |
| `segment` | ICU line breaking, Chinese segmentation, and optional hyphenation. |
| `shape` | HarfRust shaping and grapheme-safe font fallback. |
| `layout` | BiDi-aware horizontal/vertical layout, fitting, alignment, metrics, and punctuation policy. |
| `raster` | Vello draw encoding, WGPU lifecycle, target pooling, readback, and downsampling. |
| `render` | High-level facade and revision-aware prepared-page cache. |
| `types` | Shared public font and alignment value types. |
| `error` | Typed errors across scene, resource, layout, and backend boundaries. |

## Extension rules

New scene capabilities should be integrated at the narrowest stage:

- semantic interpretation and dependency discovery belong in `plan`;
- CPU resource work and Vello command recording belong in `prepare`;
- text algorithms belong in `segment`, `shape`, or `layout`;
- durable user intent belongs in `koharu-scene`, not `RenderTheme`;
- export-only or application visual policy belongs in `RenderTheme`;
- GPU lifecycle, readback, and quality controls belong in `raster`.

Adding a layer type requires a plan representation, a preparation branch, final
entity metadata, dependency tracking, and cache-invalidation tests. It must not
introduce decoded data or backend handles into `RenderPlan`.

Adding a scene field must not require renderer changes unless that field has
rendering meaning. Unknown scene extensions remain a scene/storage concern and
are ignored by this crate.

## Tests and benchmarks

The normal renderer suite covers plan compilation, cache invalidation, Unicode
segmentation, fallback shaping, BiDi ordering, fitting, vertical layout,
punctuation policy, and resource bounds:

```powershell
cargo test -p koharu-renderer
cargo clippy -p koharu-renderer --all-targets -- -D warnings
```

The ignored integration tests require platform fonts and, for raster cases, a
working GPU. They cover Japanese and Simplified Chinese horizontal/vertical
rendering, fallback symbols and emoji, Arabic and mixed BiDi text, alignment,
stroke/color paths, and GPU flow:

```powershell
cargo test -p koharu-renderer --test rendering -- --include-ignored
```

Criterion benchmarks separate text layout from reusable GPU rendering:

```powershell
cargo bench -p koharu-renderer --bench rendering
```

Visual test outputs are written under `crates/koharu-renderer/target/tests` and
are local artifacts, not durable project state.

## Current non-goals

The current renderer does not provide:

- a CPU raster backend;
- persistence of layout or render caches;
- glyph-by-glyph arbitrary polygon text flow or vertical contour flow;
- automatic spatial text-to-bubble matching;
- per-layer prepared-cache reuse inside a changed page;
- general filters, masks, blend-mode graphs, or animation; or
- an async runtime or background job scheduler.

Those features can be added behind the existing stage boundaries without
putting renderer implementation detail back into `koharu-scene`.
