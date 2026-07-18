# koharu-canvas

`koharu-canvas` is Koharu's Rust-owned editor viewport. It reads a
`koharu-scene` page, uses `koharu-renderer` for text, and renders the page and
editor overlays into a WGPU texture.

The crate implements that boundary today. `koharu-desktop` owns its production
window, webview, shared-device, and presentation integration.

## Reference

The desktop integration uses the Wry route documented by
[`koharu-desktop`](../koharu-desktop/README.md):

- a Winit parent owns the WGPU surface;
- a transparent Wry child contains the DOM UI;
- the DOM leaves the canvas rectangle transparent;
- the DOM reports that rectangle and its device-pixel ratio to Rust;
- DOM input is coalesced and forwarded through Wry IPC;
- Rust renders the viewport underneath the webview;
- the operating system composites the two native surfaces without copying
  browser pixels into Rust.

Production code separates responsibilities: `koharu-canvas` renders a canvas
texture, while `koharu-desktop` owns Wry, Winit, the window surface, event
scheduling, and final desktop composition. Its runnable `smoke` example replaces
the earlier standalone experiment.

## Scope

`koharu-canvas` owns:

- the current page view and camera;
- decoded scene images consumed by Vello's GPU atlas;
- cached per-element Vello scenes and other derived render data;
- WGPU/Vello rendering into reusable offscreen textures;
- transient viewport overlays such as selection outlines, block numbers,
  draft rectangles, guides, transform handles, and the brush cursor;
- low-latency editing of the page's single-channel text and brush masks;
- page-to-screen and screen-to-page coordinate conversion;
- scene-aware hit testing, including transform handles;
- short, explicitly scheduled image transitions.

It does not own:

- a Winit window, WGPU surface, or presentation loop;
- Wry, Tauri, WebView2, or DOM composition;
- the final fullscreen pass into the desktop surface;
- project persistence, history, or scene mutation;
- tools, gestures, selection policy, panels, shortcuts, or other UI state;
- pipeline execution, model loading, export, or network transport;
- CPU image readback for normal interactive frames.

React remains responsible for interpreting interaction: selecting a tool,
starting a drag, choosing paint or erase, editing text, and choosing the active
page. It sends typed pointer and presentation updates through the desktop host.
The canvas converts coordinates, reports what was hit, and renders immediate
feedback without deciding selection or tool policy.

Scene edits are expressed as `koharu-scene::Commands`. The caller applies them
to the `Session`, then tells the canvas about the resulting `ChangeSet`. A
finished mask stroke returns a single-channel mask snapshot and dirty region;
the caller encodes and commits it and decides whether to run the pipeline.

## Ownership boundary

```text
React in transparent Wry child
    tools, panels, gestures, text editing
                 |
                 | pointer, view, and overlay messages
                 v
koharu-desktop
    Winit event loop, Wry IPC, Session owner,
    mask commit queue, shared WGPU device/queue,
    surface and redraw scheduling
          |                         |
          | scene + interaction     | canvas texture
          v                         v
koharu-canvas ----------------> desktop surface pass
    camera, hit testing, mask editor, GPU caches,
    WGPU/Vello viewport
          |                         |
          v                         v
koharu-scene                koharu-renderer
    persistent truth             text layout and drawing

Final Wry composition: operating-system compositor
```

There is one WGPU device and one queue. `koharu-desktop` creates them and
shares them with the canvas. `koharu-canvas` must not create a second adapter,
device, queue, or headless rendering context.

## Scene model

The canvas never maintains a second editable scene graph. The active
`koharu-scene::Session` is the committed source of truth.

On page activation, the canvas reads:

- page size, required source, and optional clean/rendered page images;
- optional text, bubble, and brush masks;
- bottom-to-top `Page::elements` order;
- image blob IDs and image transforms;
- text content, style, layout, visibility, and opacity;
- the bubble mask when bubble-fitting text needs it.

Selection, hover, handles, guides, the camera, and the viewport rectangle are
presentation state and are never written to `koharu-scene`.

The canvas does not apply scene commands itself. A typical edit is:

```rust
let mut edit = session.edit();
edit.page(page)?.text(element)?.set_opacity(0.5);
let changes = edit.commit()?;

canvas.sync(&session, &changes)?;
desktop.request_redraw();
```

This keeps persistence and undo semantics in one place and makes rendering a
pure consumer of committed state.

### Existing UI feature mapping

The current UI uses older names for the same page planes. The replacement is:

| Existing UI | `koharu-scene` | Canvas use |
|---|---|---|
| source image | `Page::source` | required editable base |
| inpainted image | `PageAsset::Clean` | optional editable base |
| rendered image | `PageAsset::Rendered` | flattened preview |
| segmentation mask | `PageAsset::TextMask` | repair-mask display and editing |
| brush layer | `PageAsset::BrushMask` | brush-mask display and editing |
| text sprites | derived from `TextBlock` | rendered live by `koharu-renderer` |

Masks remain single-channel. Brush color is a presentation tint or
application setting; it is never encoded into `TextMask` or `BrushMask`. If
Koharu later needs persistent multicolored painting, that content is an image
element rather than a mask.

## Display and interaction state

Page content and editor chrome are separate inputs:

```rust
pub enum PageView {
    EditableSource,
    EditableClean,
    Rendered,
}

pub enum BaseImage {
    Source,
    Clean,
}

pub struct MaskOverlay {
    pub tint: Color,
    pub opacity: f32,
}

pub struct DisplayState {
    pub page: PageView,
    pub show_text: bool,
    pub text_mask: Option<MaskOverlay>,
    pub brush_mask: Option<MaskOverlay>,
    pub transition: Option<Duration>,
}
```

`PageView::Rendered` draws the flattened rendered artifact instead of the
editable base and live elements, preventing duplicate text. Editor chrome is
still drawn above it, while editable mask previews are ignored. Selecting
`Clean` when it is unavailable falls back to the source and reports a non-fatal
diagnostic. Selecting an unavailable rendered artifact falls back to the
current source without live elements and reports a diagnostic. Mask visibility,
tint, and opacity are presentation state and never mutate the scene.

The default transition duration for a newly available clean or rendered image
may match the current UI's 180 ms cross-fade. Transitions are bounded; they do
not create a permanent animation loop.

React owns gestures, but the canvas owns the geometry used to display them:

```rust
pub struct OverlayState {
    pub selected: Vec<ElementId>,
    pub hovered: Option<ElementId>,
    pub guides: Vec<Guide>,
    pub show_text_bounds: bool,
    pub draft: Option<Frame>,
    pub element_previews: Vec<ElementPreview>,
    pub brush_cursor: Option<BrushCursor>,
}

pub struct ElementPreview {
    pub element: ElementId,
    pub frame: Frame,
}

pub struct BrushCursor {
    pub point: PhysicalPoint,
    pub diameter: f32,
}

pub enum HitTarget {
    Handle { element: ElementId, handle: Handle },
    Element(ElementId),
}
```

Handles are tested before elements; elements are tested top-to-bottom using
the same transformed bounds and ordering as rendering. Additive selection,
which tool accepts a target, minimum block size, context menus, and deletion
remain React policy. An `ElementPreview` temporarily overrides an element's
overlay frame while it is being moved or resized; it does not mutate or
re-render the committed element until React sends the final command. The eight
handle values represent the four edges and four corners. Brush diameter is in
page pixels, so its screen-space ring follows zoom while its border remains one
physical pixel.

## Renderer contract

Interactive rendering must not call the current headless
`koharu_renderer::Renderer::composite_text` path. That path produces an
`RgbaImage`, requires GPU readback, and owns a separate WGPU context. Using it
for the canvas would add device duplication, synchronization, allocation, and
CPU upload on every changed frame.

`koharu-renderer` exposes the shared scene-encoding half as `SceneRenderer`:

```rust
pub struct SceneRenderer { /* fonts and text services */ }

impl SceneRenderer {
    pub fn new() -> Result<Self>;

    pub fn encode_text_element(
        &self,
        scene: &mut vello::Scene,
        element: &koharu_scene::Element,
        bubbles: Option<&BubbleIndex>,
        options: &PageRenderOptions,
    ) -> Result<Option<RenderedElement>>;
}
```

The invariants are:

- it appends text to the canvas's Vello scene;
- the canvas's Vello renderer uses the device and queue supplied by the host;
- it does not read back pixels;
- it does not composite a complete CPU image;
- headless export reuses this same layout and scene-encoding path before its
  optional readback step.

This is how the editor and exported result avoid duplicating typography logic.

## Public API

The API should be small and synchronous at its outer boundary:

```rust
pub struct Canvas { /* private GPU state and caches */ }

pub struct CanvasGpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

pub struct CanvasOptions {
    pub max_decoded_bytes: usize,
    pub workspace_color: Color,
    pub text: koharu_renderer::PageRenderOptions,
}

pub struct ViewState {
    pub size: PhysicalSize,
    pub camera: Camera,
    pub display: DisplayState,
}

pub enum MaskPlane {
    Text,
    Brush,
}

pub enum StrokeMode {
    Paint,
    Erase,
}

pub struct Brush {
    pub diameter: f32,
    pub mode: StrokeMode,
}

impl Canvas {
    pub fn new(
        gpu: CanvasGpu,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self>;
    pub fn new_with(
        gpu: CanvasGpu,
        options: CanvasOptions,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self>;

    pub fn show_page(&mut self, session: &Session, page: PageId) -> Result<()>;
    pub fn clear_page(&mut self);
    pub fn sync(&mut self, session: &Session, changes: &ChangeSet) -> Result<()>;

    pub fn set_view(&mut self, view: ViewState);
    pub fn set_overlays(&mut self, overlays: OverlayState);
    pub fn set_text_options(&mut self, options: PageRenderOptions);
    pub fn invalidate_fonts(&mut self);
    pub fn set_workspace_color(&mut self, color: Color);

    pub fn screen_to_page(&self, point: PhysicalPoint) -> Option<PagePoint>;
    pub fn page_to_screen(&self, point: PagePoint) -> PhysicalPoint;
    pub fn hit_test(&self, point: PhysicalPoint) -> Option<HitTarget>;

    pub fn begin_mask_stroke(
        &mut self,
        plane: MaskPlane,
        brush: Brush,
        point: PhysicalPoint,
    ) -> Result<()>;
    pub fn extend_mask_stroke(&mut self, point: PhysicalPoint) -> Result<()>;
    pub fn finish_mask_stroke(&mut self) -> Result<Option<MaskCommit>>;
    pub fn cancel_mask_stroke(&mut self);
    pub fn acknowledge_mask_commit(
        &mut self,
        page: PageId,
        plane: MaskPlane,
        generation: u64,
        blob: BlobId,
    ) -> Result<()>;

    pub fn render(&mut self, now: Instant) -> Result<CanvasFrame<'_>>;
    pub fn take_diagnostics(&mut self) -> Vec<CanvasDiagnostic>;
}

pub struct CanvasFrame<'a> {
    pub texture: &'a wgpu::TextureView,
    pub size: PhysicalSize,
    pub generation: u64,
    pub needs_redraw: bool,
}

pub struct MaskCommit {
    pub page: PageId,
    pub plane: MaskPlane,
    pub dirty: PixelRect,
    pub generation: u64,
    /* private immutable mask snapshot */
}

impl MaskCommit {
    pub fn size(&self) -> PixelSize;
    pub fn encode_png(&self) -> Result<Vec<u8>>;
}
```

`show_page` and `sync` read the referenced immutable bytes from the session,
but image decoding is dispatched to Rayon's bounded worker pool. The supplied
`wake` callback asks the desktop event loop
for a redraw when a resource becomes ready. `render` drains completed work but
performs no SQLite access, image decoding, font downloads, or CPU readback.
There is no caller-visible `prepare_image` phase. Blob IDs are content hashes,
so late results remain safe cache entries; a result is installed into page or
mask state only when the active page still references that ID. The scene store
has already validated the required source before `show_page` can read it.
Optional resources appear independently and may cross-fade in.

`CanvasFrame` borrows a canvas-owned texture and is valid until the next
resize, device recreation, or render that replaces the target. The desktop
host consumes it immediately in its surface pass. `needs_redraw` is true only
while a bounded transition is active; the host requests another frame and
stops when it becomes false.

### Mask editing

Mask editing is an interaction primitive, not a tool or pipeline operation.
React chooses the plane, diameter, and paint/erase mode. The canvas converts
pointer samples to page coordinates, draws round joined strokes, clips them to
the page, and updates a tiled one-byte-per-pixel buffer. Only non-empty or
changed tiles are converted to cached tinted Vello image data, so an empty 4K
mask does not allocate a 4K RGBA preview. The mask is never read back from the
GPU. Starting a stroke on an absent mask creates a zero-filled page-sized
plane; an existing mask must finish loading before editing can begin.

`finish_mask_stroke` returns a cheap immutable snapshot that shares unchanged
tiles with the live mask. Its dirty rectangle is expressed in page pixels and
the commit is `Send`. `MaskCommit::encode_png` is pure and must run on a
background worker. The desktop host serializes commits per page and mask
plane, encodes them as single-channel PNG, and applies the corresponding
`Commands::set_asset`. After applying it, the host passes the resulting
`BlobId` to `acknowledge_mask_commit` before `sync`. The generation prevents an
older completed encode from replacing or visually reverting a newer stroke.
Pipeline execution, including regional inpainting, starts only after the
application accepts the scene commit.

Cancel restores the tiles touched by the active stroke. External scene changes
replace a mask only when it has no newer uncommitted local generation;
otherwise the host resolves the conflict before calling `sync`. This transient
mask buffer is not a second scene graph and is discarded on page close.

## View and coordinates

The coordinate spaces are explicit:

```text
DOM logical pixels -- koharu-desktop + DPR --> physical viewport pixels
physical viewport pixels -- inverse camera --> page coordinates
page coordinates -- camera --> canvas texture coordinates
```

`koharu-canvas` accepts physical pixels only. Wry logical coordinates and
device-pixel ratio conversion belong to `koharu-desktop`.
The camera stores translation and zoom in `f64` to avoid visible jitter during
large zooms and pans; scene geometry remains the `f32` values defined by
`koharu-scene`.

The camera is one affine transform and its inverse. Fit-page, actual-size, and
zoom-around-pointer are constructors or methods on `Camera`, not separate
rendering paths. A resize changes only the output target and camera viewport;
it never mutates the scene.

`screen_to_page` and `page_to_screen` expose that exact transform to the desktop
interaction bridge, so block drafting, brush painting, context menus, and hit
testing never duplicate camera math in React. Points outside a zero-sized
viewport return `None`; stroke operations clip otherwise valid points to the
page.

Hit testing uses the same inverse transform and element ordering as rendering.
It checks visible transform handles first and visible elements from top to
bottom. Rotated frame bounds use the element angle; glyph- or alpha-precise hit
testing should be added only when a real interaction requires it.

React may retain the current fit, actual-size, wheel, pinch, pan, and scrollbar
policies. It sends camera updates through the desktop host. Pointer motion may
be coalesced to one UI update per event-loop wake, but a paint message carries
all ordered samples collected in that interval so a fast stroke has no gaps.

## Rendering

Editable content is built in this order:

1. clear the viewport to the workspace background and clip page content to the
   page bounds;
2. apply the page-to-canvas camera transform;
3. draw the selected source or clean base image;
4. draw enabled text- and brush-mask previews with their presentation tint and
   opacity;
5. traverse `Page::elements` once from bottom to top, drawing image elements
   directly and, when live text is enabled, asking `koharu-renderer` to append
   each text element at its exact position in that same traversal.

Rendered-preview mode instead draws only `PageAsset::Rendered` as the page
content. Both modes then draw editor chrome in this order:

1. page-space guides, numbered text bounds, and the block draft;
2. selection and hover outlines;
3. screen-space handles and brush cursor with constant physical-pixel sizes.

Image and text elements must never be rendered in separate type-grouped
passes: doing so would violate the mixed element order stored by
`koharu-scene`. The persisted rendered-page artifact is also not an editable
base image because drawing live elements over it would duplicate content.

Page images are textured quads, text comes from `koharu-renderer`, and editor
overlays are ordinary Vello geometry. Internally the canvas uses two reusable
viewport-sized targets:

- a content target containing the current page view;
- the published output target, which samples the content target and adds
  editor chrome.

An overlay-only update therefore does not traverse elements, rebuild text, or
redraw page content. A mask stroke invalidates the content target, while cached
image data and per-element Vello scenes are reused; text is not laid out again.
Only mask tiles whose pixels or tint changed regenerate their RGBA preview.
The second target is an intentional small memory cost for low-latency cursor,
drafting, dragging, and handle feedback.

The output contract used by `koharu-desktop` is:

- format: `wgpu::TextureFormat::Rgba8Unorm`;
- usage: `STORAGE_BINDING | TEXTURE_BINDING | COPY_SRC`;
- RGB values: display-referred sRGB values stored in the unorm texture;
- size: the canvas rectangle in physical pixels, never the full DOM window;
- background: opaque for the initial implementation.

When sampling this texture into an sRGB swapchain, `koharu-desktop` performs the
required sRGB-to-linear conversion in
[`present.wgsl`](../koharu-desktop/src/present.wgsl). That fullscreen surface
pass and all surface error handling belong to `koharu-desktop`, not this crate.

## Invalidation and scheduling

The canvas records why a frame is dirty:

- `Target`: physical size or device changed;
- `View`: camera or displayed page mode changed;
- `Scene`: visible page content changed;
- `Mask`: mask pixels or mask presentation changed;
- `Overlay`: selection, hover, guides, or handles changed;
- `Resource`: an image or bubble index became available. The host explicitly
  calls `invalidate_fonts` after changing installed or cached fonts.

Multiple changes before the next event-loop wake produce one redraw request.
The canvas does not run a permanent animation loop. `koharu-desktop` owns the
`EventLoopProxy`, `request_redraw`, and the `redraw_requested` coalescing flag.

`ChangeSet` makes scene invalidation incremental. Changes to another page do
not invalidate the current canvas. Camera and overlay changes never reload
scene blobs or rebuild text layout. Overlay dirtiness redraws only the output
target. A transition keeps requesting frames only until its configured duration
has elapsed.

## Caches and performance

The performance contract is:

- no browser-frame transfer;
- no GPU-to-CPU readback in interactive rendering;
- no CPU compositing of a complete page;
- no second WGPU device;
- no image decode or SQLite query inside `render`;
- no render at full page resolution when the visible viewport is smaller;
- no continuous redraw while the viewport is idle.

The canvas keeps bounded caches for:

- decoded images keyed by immutable `BlobId`;
- encoded element scenes keyed by the complete element value and bubble mask;
- bubble-mask indexes keyed by mask `BlobId`;
- single-channel mask planes backed by copy-on-write tiles and lazily tinted
  Vello image tiles;
- content and output targets, recreated only after a physical-size change.

Panning and zooming reuse decoded images and encoded element scenes. A scalar
scene edit re-encodes only the affected element before rebuilding the cheap
combined ordering scene. Replacing a blob decodes only the new blob. Pointer
motion updates only editor chrome; mask painting copies only touched tiles.
Full-mask PNG encoding happens after a stroke on a background worker because
the immutable scene blob necessarily represents the complete plane.

Decoded-image and bubble-index cache limits are byte-based and configurable.
Entries used by the active page are pinned for the frame; least-recently-used
entries from inactive pages may be evicted. The active page's copy-on-write
mask tiles are bounded by its scene-validated dimensions and are discarded on
page change. Device loss drops all GPU resources while leaving the scene
untouched.

## Failure and lifecycle rules

- A missing required source fails `show_page` before changing the active page.
  `koharu-scene` validates image bytes and dimensions before they can become a
  stored source.
- A missing or invalid optional clean image, rendered image, mask, image
  element, or font produces a diagnostic and omits or falls back from only that
  resource. It does not make the rest of the page unusable.
- Zero-sized targets suspend rendering without destroying scene caches.
- Surface loss, timeout, and presentation are not canvas errors because the
  canvas owns no surface.
- Device loss invalidates the canvas. `koharu-desktop` recreates the shared
  device and constructs a new canvas.
- Resource and font diagnostics include the affected page, element, or asset.
- A frame is published only after all command encoding succeeds; callers never
  receive a partially updated output texture as a new generation.
- Leaving a page cancels its active stroke. Pending immutable mask commits may
  finish in the host, but generation checks prevent them from overwriting newer
  state.

## Testing

The crate currently has unit coverage for camera inversion,
zoom-around-pointer, rotated bounds, dirty rectangles, handle ordering,
copy-on-write mask cancel, single-channel PNG output, and stale mask commit
acknowledgement. Desktop integration should add:

- in-memory `koharu-scene::Session` fixtures for incremental `ChangeSet`
  invalidation;
- headless WGPU tests for source, clean and rendered views, both mask previews,
  image elements, text, vertical text, transforms, opacity, and overlays;
- mask-edit tests for paint, erase, page-edge clipping, round joins, cancel,
  dirty-region tracking, tinted-tile regeneration, single-channel PNG output,
  and commit generations;
- interaction tests for block drafts, numbered text bounds, multi-selection
  handles, brush cursor sizing, and screen/page coordinate round trips;
- golden comparisons between canvas text and headless export output;
- resize tests including zero-sized and large physical targets;
- deterministic transition tests using an injected `Instant`;
- cache tests proving unchanged blobs are not decoded or re-encoded again;
- release benchmarks for a representative 4K page during idle, pan, zoom,
  text edit, image replacement, page switch, cursor motion, and mask painting.

Desktop integration tests for transparent Wry composition, focus, IME,
non-integer DPR conversion, multiple monitors, surface recovery, ordered
pointer-sample batching, mask commit serialization, and packaging belong to
`koharu-desktop`.

## Implementation status

Implemented here: the shared-device no-readback renderer path, camera and two
WGPU targets, asynchronous blob decoding, all page display modes, mixed-order
text/image composition, overlays and hit testing, tiled mask editing,
generation-safe commits, bounded transitions, and byte-bounded image caches.

`koharu-desktop` now presents this texture in the Wry route, translates logical
DOM coordinates to physical pixels, coalesces redraws, serializes mask commits,
and handles ordinary surface recovery. Device loss remains fatal in the first
desktop implementation.

The first milestone is complete when a page from an in-memory `Session` renders
to a WGPU texture with no readback; all existing UI page planes can be selected
or overlaid; block drafting, selection handles, and mask strokes have immediate
feedback; and an overlay-only update redraws without decoding, uploading,
relaying out, or traversing unchanged page content.
