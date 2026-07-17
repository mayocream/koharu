# koharu-scene

> [!IMPORTANT]
> This crate is a greenfield rewrite. Existing APIs and project files are not
> compatibility constraints.

`koharu-scene` is a native Rust library for an editable 2D scene graph stored in
SQLite. It owns pages, ordered node trees, image and mask blobs, commands,
history, checkpoints, and change reports.

It does not own fonts, rendering, UI interaction, pipelines, networking, or
application workflow state. The crate is synchronous, uses `rusqlite` directly,
and does not compile to WASM.

## Design rules

1. Every scene element is a `Node`; “layer” is an application-level view.
2. The arena is the only hierarchy authority.
3. `CommandBatch` is the only mutation language.
4. The fluent API is syntax sugar over `CommandBatch`.
5. Callers attach image bytes directly to image or mask commands.
6. SQLite stores each immutable blob once and each edit as one small commit.
7. `Session` is synchronous and belongs on a scene worker, not a UI thread.
8. Rendering and application concepts remain outside the crate.

## Scope

`koharu-scene` owns:

- ordered pages and node trees;
- group, mask, image, and text nodes;
- transforms, visibility, opacity, and text semantics;
- encoded image and single-channel alpha-mask blobs;
- atomic reversible commands, revisions, and change reports;
- competing-writer detection and idempotent retries;
- SQLite schema versioning, checkpoints, history pruning, blob garbage
  collection, backup, and recovery;
- disk-backed and in-memory sessions.

Callers own:

- font discovery, loading, fallback, and licensing;
- rendering, compositing, text shaping, and rasterization;
- decoded-image, glyph, texture, tile, and preview caches;
- selection, tools, gestures, snapping, and viewport state;
- pipelines, jobs, translation workflows, and application roles;
- async scheduling, transport, authentication, and UI DTOs.

There is no arbitrary extension map. Application state should not be hidden in
the scene merely because it needs persistence.

## Architecture

```text
       owned CommandBatch                 borrowed reads
              |                                |
              v                                v
       +---------------------------------------------+
       |                   Session                   |
       |              Scene + SQLite                 |
       +---------------------------------------------+
            |                 |                |
         commits          checkpoints         blobs

       scene worker                         caller-owned
       serializes writes              renderer + font manager
```

There are four central public types:

| Type | Responsibility |
| --- | --- |
| `Scene` | Immutable-to-callers view of committed pages and nodes. |
| `Session` | Keeps the in-memory scene and SQLite head synchronized. |
| `CommandBatch` | Owned description of one atomic edit. |
| `ChangeSet` | Precise information needed to synchronize consumers. |

`Session` exposes `&Scene`, never `&mut Scene`. Internally it uses ordinary
mutable collections. Immutability is an API and transaction guarantee, not a
persistent-collection implementation.

## Scene and hierarchy

```rust
pub struct Scene {
    revision: Revision,
    pages: IndexMap<PageId, Page>,
    nodes: HashMap<NodeId, NodeLocation>,
}

pub struct Page {
    id: PageId,
    name: String,
    size: CanvasSize,
    tree: indextree::Arena<TreeEntry>,
    root: indextree::NodeId,
}

enum TreeEntry {
    PageRoot,
    Node(Node),
}

struct NodeLocation {
    page: PageId,
    handle: indextree::NodeId,
}
```

`IndexMap` supplies semantic page order and expected O(1) page lookup. Each page
owns one arena with a private sentinel root. The global `nodes` index supplies
expected O(1) lookup by stable `NodeId`.

The arena is the only hierarchy authority. `Node` has no parent field and
container nodes have no children vector. Parent and sibling links therefore
cannot disagree with a second representation.

Arena handles are process-local and never persisted or exposed. Public IDs are
opaque and stable:

```rust
pub struct PageId(Uuid);
pub struct NodeId(Uuid);
pub struct CommandId(Uuid);
pub struct Revision(u64);
pub struct BlobId([u8; 32]);
```

## Nodes

```rust
pub struct Node {
    id: NodeId,
    name: Option<String>,
    visible: bool,
    opacity: f32,
    transform: Transform,
    kind: NodeKind,
}

pub enum NodeKind {
    Group,
    Mask(MaskNode),
    Image(ImageNode),
    Text(TextNode),
}

pub struct MaskNode {
    blob: BlobId,
    natural_size: PixelSize,
}

pub struct ImageNode {
    blob: BlobId,
    natural_size: PixelSize,
}

pub struct TextNode {
    text: String,
    style: TextStyle,
    layout: TextLayout,
}
```

Groups and masks are containers. Images and text are leaves. A mask's
single-channel alpha image applies to its complete child subtree. This gives a
mask an unambiguous structural target without a second graph edge. The scene
stores this meaning; `koharu-renderer` performs the actual compositing.

Siblings are stored back-to-front, so the final sibling is visually above
earlier siblings. An image is an image node; whether the application presents
it as a layer is outside the scene model.

Transforms are finite 2D affine transforms relative to the parent. Opacity is
finite and constrained to `0..=1`. World transforms, effective opacity, and
active mask stacks are derived during traversal and never persisted.

### Text

`TextNode` stores editable text, a uniform Photoshop-style `TextStyle`, and its
frame constraints. `TextStyle` includes:

- an ordered font-family fallback list, font size, weight, stretch, and slant;
- color, line height, letter and word spacing, glyph scaling, and baseline
  shift;
- text-local rotation, horizontal and vertical alignment, writing mode,
  underline, and strikethrough;
- an ordered effect stack supporting strokes, inner and drop shadows, inner and
  outer glows, bevel/emboss, satin, color overlays, and gradient overlays.

`TextLayout` owns optional frame width and height, frame insets, and overflow
behavior. Text-local rotation is separate from the node transform: it rotates
laid-out glyphs inside the text frame, while `Node::transform` positions the
whole node in its parent.

Font-family names and face characteristics are scene semantics, but font files
remain outside the scene. The renderer resolves the stored preference list,
performs fallback and shaping, and owns glyph and GPU caches. A text node
contains no font bytes, rasterized text, shaped glyphs, fallback result, glyph
atlas, or GPU object. Reproducible output therefore requires the same fonts to
be available to the renderer.

## Commands

`CommandBatch` is an opaque owned builder. Its public methods are typed scene
operations such as creating, removing, moving, and updating nodes. Its private
representation contains:

- commands expressed with stable page and node IDs;
- attached bytes keyed deterministically by `BlobId`;
- the exact base revision and a unique `CommandId`.

The low-level API is usable without a `Session` borrow, so another thread may
build a batch from a known revision:

```rust
let mut commands = CommandBatch::new(revision);
let page = commands.create_page(Page::new("Page 1", canvas))?;
let background = commands.create(
    Parent::Page(page),
    Position::Top,
    node::image(background_bytes)
        .named("Background")
        .at(background_transform),
)?;
commands.set_opacity(background, 0.5)?;

let applied = session.apply(commands)?;
```

`Position` uses editor terminology—`Top`, `Bottom`, `Above(NodeId)`, and
`Below(NodeId)`. Numeric indices and a generic public `insert_before` operation
are deliberately absent. The private normalized command uses a stable sibling
anchor. Pages use the parallel `PagePosition::{First, Last, Before, After}` API.

Node construction helpers such as `node::image(bytes)` return an opaque command
input, not a staged `Node`. Committed `Node` values always contain a `BlobId` and
never contain pending bytes.

Commands in a batch are interpreted in order, so later commands may refer to
pages or nodes created earlier in the same batch. Each builder method either
appends one complete command or leaves the batch unchanged on error.

### Fluent API

The fluent API delegates to the same `CommandBatch` methods:

```rust
let mut edit = session.edit();

let page = edit.create_page(Page::new("Page 1", canvas))?;
let background = edit.page(page)?.create(
    node::image(background_bytes)
        .named("Background")
        .at(background_transform),
)?;
let caption = edit.page(page)?.create(
    node::text("Hello", text_style, text_layout)
        .named("Caption")
        .at(caption_transform),
)?;

edit.page(page)?.node(caption)?.set_opacity(0.5)?;
edit.page(page)?.text(caption)?.set_text("Hello world")?;
edit.page(page)?.node(caption)?.place_above(background)?;

let applied = edit.commit()?;
```

`PageEdit`, `NodeEdit`, and typed handles borrow one batch builder. They cannot
outlive it or commit independently. Despite the name `edit`, they never provide
mutable access to the committed scene. Direct and fluent construction produce
the same canonical stored command.

## Blob storage

The caller attaches encoded bytes directly when creating an image or mask or
when replacing its content:

```rust
let image = edit.page(page)?.create(node::image(encoded_image))?;
let mask = edit.page(page)?.create(node::mask(encoded_alpha_mask))?;
```

There is no separate image-store insertion API. Internally:

1. the command helper accepts `Arc<[u8]>` without cloning the payload;
2. it computes `BlobId` as a BLAKE3 hash of the encoded bytes;
3. it keeps one pending attachment per ID in the batch;
4. validation reads metadata and derives `PixelSize`;
5. `apply()` inserts a missing blob in the same transaction as the commit;
6. stored commands and checkpoints contain only `BlobId` and metadata.

Blob reads are flat and lazy:

```rust
let image = session.scene().page(page)?.image(image_id)?;
let encoded = session.blob(image.blob())?;
```

`Session::blob` loads the complete encoded blob into `Arc<[u8]>`. Configured
blob-size limits bound this allocation. Decoded images and GPU resources remain
renderer caches. Fonts are never stored as scene blobs.

Image attachment validates the container metadata, dimensions, and configured
limits but does not fully decode pixels. A mask must report a single-channel
color type. Full decode errors remain renderer errors. This keeps scene commits
predictable and avoids duplicate decoding.

## Reading and change reports

Reads borrow committed data without cloning nodes:

```rust
let scene = session.scene();
let page = scene.page(page_id)?;
let caption = page.text(caption_id)?;

println!("{}", caption.text());

for event in page.walk() {
    renderer.update(event);
}
```

Traversal is iterative and emits container enter/exit events, allowing a
renderer to maintain transform, opacity, and mask stacks without independently
walking every node's ancestors. Direct child iteration also borrows the arena
and does not allocate a temporary ID vector.

`ChangeSet` records its starting and ending revisions and contains IDs and
flags, never blobs or application events. It reports:

- created and removed pages and nodes;
- reordered parents and moved nodes;
- field changes such as transform, opacity, text, or blob;
- dirty subtree roots when inherited transform, visibility, opacity, or masking
  changed.

Removed-node lists include descendants because they cannot be queried from the
new scene. Dirty subtree roots avoid eagerly enumerating descendants for a
simple ancestor update. Consumers may combine `ChangeSet` with their previous
cache to invalidate old bounds. `ChangeSet::requires_reload()` is true when
`refresh()` had to load a checkpoint and therefore cannot report an exact
incremental delta.

```rust
pub struct Applied {
    pub revision: Revision,
    pub changes: ChangeSet,
    pub already_applied: bool,
}
```

## Session and synchronous execution

```rust
impl Session {
    pub fn create(path: impl AsRef<Path>, config: SessionConfig) -> Result<Self>;
    pub fn open(path: impl AsRef<Path>, config: SessionConfig) -> Result<Self>;
    pub fn memory(config: SessionConfig) -> Result<Self>;

    pub fn revision(&self) -> Revision;
    pub fn scene(&self) -> &Scene;
    pub fn blob(&self, id: BlobId) -> Result<Arc<[u8]>>;

    pub fn refresh(&mut self) -> Result<ChangeSet>;
    pub fn edit(&mut self) -> Edit<'_>;
    pub fn apply(&mut self, batch: CommandBatch) -> Result<Applied>;
    pub fn revert(&mut self, revisions: &[Revision]) -> Result<Applied>;

    pub fn checkpoint(&mut self) -> Result<()>;
    pub fn prune_history(&mut self, keep_from: Revision) -> Result<GcReport>;
    pub fn backup(&self, path: impl AsRef<Path>) -> Result<()>;
}
```

`rusqlite` and image hashing are blocking. `Session` therefore belongs to one
scene worker thread. Other threads may build owned `CommandBatch` values and
send them to that worker. An application that needs async ergonomics should put
an async channel-based handle around the worker; making the core API async would
only hide blocking work behind `spawn_blocking`.

`refresh()` replays commits after the local revision. If history needed by the
local session was pruned, it reloads the active checkpoint and returns a change
set whose `requires_reload()` flag is true. Otherwise it returns one precise,
merged `ChangeSet` for the imported revisions.

SQLite serializes writes. Exact base revisions prevent lost updates; edits to
unrelated nodes can still conflict. The crate intentionally does not implement
automatic merge or a CRDT. It is optimized for one application scene worker
with safe recovery from occasional additional-process writes, not sustained
multi-process write throughput.

## Atomic apply

`Session::apply` follows one path for direct and fluent commands:

1. require the local session to match SQLite's durable head, otherwise return
   `StaleSession` and let the caller invoke `refresh()`;
2. normalize attachment metadata and compute a deterministic request hash;
3. check for an identical retained `CommandId` and request-hash retry;
4. require the batch's base revision to equal the current revision;
5. materialize and temporarily apply canonical reversible commands while
   collecting a `ChangeSet` and blob references;
6. validate the resulting scene, remove unchanged operations, and apply those
   same commands backward to restore the public scene;
7. start `BEGIN IMMEDIATE`, repeat the retry lookup, and recheck the head;
8. insert missing blobs, append the commit, and advance the head;
9. commit SQLite;
10. reapply the already-validated stored commands to memory and return
    `Applied`.

Temporary mutation happens inside the synchronous `&mut Session` call and is
not observable by callers. Backward application restores the scene on every ordinary
pre-commit error, avoiding both a full-scene clone and a second shadow hierarchy.
After the SQLite commit, replay cannot encounter input validation or database
errors because it uses the stored commands already exercised in step 5. An
unexpected internal failure after commit poisons the `Session`; reopening it
reconstructs the authoritative state from SQLite. Ordinary errors leave both
SQLite and the public scene unchanged.

An identical retained retry returns the original result with
`already_applied = true`. Reusing a retained command ID for different canonical
commands is an error. If another writer wins the race after validation,
`RevisionConflict` is returned; the caller refreshes and rebuilds the batch.
If the other writer applied the identical command, this session rolls back its
transaction, refreshes to the durable head, and then returns the stored result.

## SQLite format

The database contains three logical tables:

```text
project
    singleton, schema_version, project_id,
    head_revision, checkpoint_revision NULL

commits
    revision PRIMARY KEY, parent_revision,
    command_id UNIQUE, command_hash,
    forward_batch, blob_refs, checkpoint NULL

blobs
    id PRIMARY KEY, bytes
```

`project` provides direct O(1) access to the durable head and active checkpoint.
`commits` is the scene log, retry record, undo history, and checkpoint store.
Each stored operation contains both its expected and replacement values, so its
inverse and `ChangeSet` are derived rather than stored again. `blob_refs` is a
sorted unique list covering both directions and checkpoint data; it lets garbage
collection avoid decoding complete commands.

`blobs` is immutable and content-addressed. It uses a BLOB primary key and
`WITHOUT ROWID`; inserting an existing ID writes no duplicate payload. A normal
metadata edit appends one commit row and updates the singleton head. It does not
rewrite the scene or its blobs.

There are no page, node, property, font, receipt, or reference tables. The
SQLite schema version gates the Postcard encodings, whose enum order is part of
that schema version. Canonical encoders sort maps and sets and never serialize
arena handles. `command_hash` covers the normalized request and referenced blob
IDs before scene-dependent no-op removal, so retry identity remains stable. It
never depends on hash-map iteration order.

Disk sessions enable WAL, durable synchronization, and a bounded busy timeout.
In-memory SQLite uses its supported memory journal rather
than pretending to provide WAL behavior. WAL and locking tests therefore use a
temporary disk database.

Backup and Save As use SQLite's backup API rather than copying an open database
file. The resulting file includes scene history and stored image and mask blobs.

## Checkpoints, undo, and garbage collection

Opening a scene:

1. validates the schema and singleton row;
2. loads `project.checkpoint_revision`, or starts at empty revision zero;
3. rebuilds arena handles and the global node index from the checkpoint;
4. replays later commits in revision order;
5. verifies the revision chain, final hierarchy, and referenced blob IDs.

It reads blob IDs during open but does not read image payloads.

Checkpoint creation serializes the current scene without blob bytes and updates
the checkpoint row and `project.checkpoint_revision` atomically. By default a
commit creates a checkpoint every 1,024 revisions, bounding later replay to at
most 1,023 commits. `SessionConfig::checkpoint_interval` can tune or disable this,
and `checkpoint()` remains available for explicit save or maintenance points.
Only a checkpointing commit pays the O(scene size) serialization cost.

Every stored operation contains preconditions and both values needed to apply it
forward or backward. `revert(revisions)` derives the requested inverse operations
in newest-to-oldest order and applies them as one new commit. A changed
precondition returns `HistoryConflict` rather than overwriting later work. The
application owns undo grouping and labels; the scene owns safe reversible data.

`prune_history(keep_from)` first checkpoints the current head. It retains commit
rows from `keep_from` for retry and undo, deletes older records, then
garbage-collects blobs in the same maintenance transaction. It marks IDs from
the current scene and retained `blob_refs`, then deletes every unmarked blob
row. Its memory use is proportional to unique blob IDs, not payload bytes.

Pruning also defines the retry guarantee: a `CommandId` is idempotent only while
its commit is retained. Deleting blobs makes SQLite pages reusable but does not
shrink the file immediately. Optional offline `VACUUM` is separate from normal
garbage collection.

## Validation

Validation rejects:

- duplicate page, node, or retained command IDs;
- missing, unreachable, or multiply placed nodes;
- cycles, self-parenting, and children under image or text leaves;
- placement anchors outside the destination container;
- non-finite transforms, geometry, text values, or opacity;
- zero page or image dimensions;
- unsupported image headers or metadata;
- masks whose metadata is not single-channel;
- attachments whose BLAKE3 digest does not match their `BlobId`;
- blob references that are neither already stored nor attached to the batch;
- stale base revisions;
- configured limits on pages, nodes, encoded blob bytes, width, height,
  decoded pixel count, batch attachment bytes, command count, text bytes, font
  families, effects, and gradient stops.

Hierarchy validation and traversal are iterative, so deeply nested input does
not consume the Rust call stack. There is no arbitrary depth limit; the node
limit bounds hierarchy memory while avoiding a full depth scan on scalar edits.

## Performance contract

| Operation | Expected cost |
| --- | --- |
| Page or node lookup | Expected O(1) |
| Read one node | Expected O(1), allocation-free |
| Insert a metadata-only leaf | Expected O(1) after validation |
| Reparent within a page | O(depth) validation, O(1) link update |
| Remove or cross-page move | O(subtree size) |
| Set a scalar field | Expected O(1) |
| Walk a page | O(page nodes) |
| Metadata-only durable commit | O(commands plus affected subtrees) |
| Attach a blob | O(encoded bytes) for metadata, hashing, and at most one write |
| Read a blob | O(encoded bytes), performed lazily |
| Refresh | O(new commit effects) |
| Open | O(checkpoint nodes plus later commands and referenced blob IDs) |
| Checkpoint | O(scene size), excluding blob bytes |
| Prune and blob GC | O(scene size plus retained blob references and blob rows) |

Additional rules:

- A non-checkpointing edit never clones or serializes the complete scene.
- Opening and traversing a scene never read blob payloads or fonts.
- Pending payloads use `Arc<[u8]>`; command construction does not clone them.
- History and checkpoints contain blob IDs, never duplicate payloads.
- Open replays commit rows as a stream rather than retaining the complete log.
- Image metadata support excludes encoder-only and Rayon features.
- Repeated lookup and replay queries use rusqlite's prepared-statement cache.
- Unchanged writes are canonicalized away before opening a transaction.
- Pointer-move previews remain transient application state; a drag normally
  commits once when it ends.

## Testing

`Session::memory` uses SQLite `:memory:` with the same schema, validation, and
command path as disk sessions. Tests for WAL, backup, locking, and crash recovery
use `tempfile` with `Session::create`.

Required tests include:

- arbitrary insert, remove, reorder, reparent, and mask-container sequences;
- direct and fluent APIs producing identical canonical commits;
- multi-command references to pages and nodes created earlier in a batch;
- failed batches leaving the scene, head, commit log, and blob table unchanged;
- deterministic command encoding and idempotent retries;
- stale-session refresh and competing-session conflicts;
- backward-application preconditions, grouped reverts, and history pruning;
- blob hashing, deduplication, missing-reference detection, and garbage
  collection;
- image metadata, size limits, and single-channel mask validation;
- crash/reopen around transaction boundaries;
- checkpoint replay and backup retaining every reachable blob;
- disk and memory sessions producing identical scene results.

The Criterion benchmark target currently measures traversal at 1k, 10k, and
100k nodes and scalar commits in a 100k-node scene. Future benchmarks should add
deep subtree operations, long-log open and refresh, blob attachment, checkpoint,
garbage collection, and backup workloads.

## Module layout

```text
src/
  lib.rs
  error.rs
  id.rs
  geometry.rs
  style.rs
  blob.rs
  node.rs
  scene.rs
  command.rs
  edit.rs
  session.rs
  storage.rs
```

The scene, blob store, and SQLite history remain in one crate. There is no font,
renderer, network, async runtime, or generic repository module.

## References

- [`indextree`](https://docs.rs/indextree/latest/indextree/) supplies the ordered
  arena hierarchy and stale-handle detection.
- [`indexmap`](https://docs.rs/indexmap/latest/indexmap/) supplies ordered page
  lookup.
- [`rusqlite`](https://docs.rs/rusqlite/latest/rusqlite/) is the only database
  API.
- [SQLite transactions](https://www.sqlite.org/lang_transaction.html),
  [WAL](https://www.sqlite.org/wal.html), and
  [backup](https://www.sqlite.org/backup.html) provide the durability and
  concurrency foundations.
- [Graphite's graph storage](https://github.com/GraphiteEditor/Graphite/tree/master/document/graph-storage)
  demonstrates useful separation between runtime state, operation history, and
  resources. Koharu keeps a direct node hierarchy and linear SQLite history
  instead of a procedural graph, CRDT, or multi-file working copy.
