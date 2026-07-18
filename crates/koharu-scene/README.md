# koharu-scene

`koharu-scene` is Koharu's small, native manga-document engine. It owns the
editable project model, atomic commands, undo data, SQLite persistence, and
content-addressed image blobs.

This is a greenfield format. Existing `koharu-scene` APIs and project files are
not compatibility constraints.

## Scope

The crate is deliberately Koharu-specific. It models exactly the persistent
state needed to import manga pages, detect and edit text regions, run OCR and
translation, inpaint, render, add user image overlays, export, undo, and reopen
a project.

It does not model a general graphics editor. There are no arbitrary groups,
container masks, paths, generic node properties, extension maps, pipeline model
objects, renderer caches, or UI interaction state.

The ownership boundary is:

- `koharu-scene`: projects, pages, text blocks, image overlays, named page
  artifacts, commands, history, blobs, and SQLite;
- `koharu-pipeline`: scheduling, models, stages, provenance, and transient
  processor outputs;
- `koharu-renderer`: fonts, shaping, compositing, text rasters, and GPU caches;
- React/application code: selection, tools, gestures, viewport, and transport.

The crate is synchronous. A desktop application should keep its `Session` on a
scene worker rather than a UI thread.

## Document model

The scene is a fixed two-level graph:

```text
Project
└── Page[]
    ├── source image
    ├── page artifacts
    │   ├── clean image
    │   ├── rendered image
    │   ├── text mask
    │   ├── bubble mask
    │   └── brush mask
    └── Element[]                 bottom to top
        ├── Text(TextBlock)
        └── Image(ImageElement)   user overlay
```

Its public shape is intentionally direct:

```rust
pub struct Project {
    pub pages: Vec<Page>,
}

pub struct Page {
    pub id: PageId,
    pub name: String,
    pub size: Size,
    pub source: BlobId,
    pub assets: PageAssets,
    pub elements: Vec<Element>,
}

pub struct Element {
    pub id: ElementId,
    pub frame: Frame,
    pub visible: bool,
    pub opacity: f32,
    pub kind: ElementKind,
}

pub enum ElementKind {
    Text(TextBlock),
    Image(ImageElement),
}
```

`Vec` order is semantic: pages are reading order and elements are bottom-to-top
paint order. `Session` builds private hash indexes after opening, providing
expected O(1) lookup without persisting a second ordering representation.

### Why artifacts are page fields

Source, clean, rendered, and masks are pipeline-wide page planes. Treating them
as ordinary user nodes creates role searches, duplicate-role validation, and
meaningless transforms and stacking choices. Named optional fields make their
cardinality and purpose explicit.

The source is required and determines page dimensions. Clean/rendered images
and all masks must have the same dimensions. Masks must be encoded as
single-channel images.

`PageAsset` exists only to select one of these fields in a command. It is not a
generic layer role.

### Why only two element kinds

Koharu users directly arrange editable text blocks and imported image overlays.
They do not need a general hierarchy. Keeping one flat visual stack removes
parent IDs, arena handles, traversal events, cycle checks, subtree operations,
and ambiguous mask ownership.

### Text blocks

```rust
pub struct TextBlock {
    pub source: Option<SourceText>,
    pub translation: Option<String>,
    pub style: TextStyle,
    pub layout: TextLayout,
}

pub struct SourceText {
    pub text: String,
    pub language: Option<String>,
    pub direction: TextDirection,
    pub confidence: Option<f32>,
    pub lines: Vec<Quad>,
}
```

This is enough for detection geometry, OCR, translation, manual correction,
typesetting, and export. A detector's name and model configuration remain
pipeline metadata. Font detection is converted into editable `TextStyle`
instead of storing a model-specific prediction object.

`TextStyle` stores semantic Photoshop-style typography and effects, including
font fallback families, size, weight, stretch, slant, spacing, scaling,
baseline shift, text-local angle, decorations, strokes, shadows, glows, bevel,
satin, and overlays. `TextLayout` stores alignment, writing mode, insets,
overflow, and whether layout fits the original frame or its bubble.

Font bytes, shaped glyphs, rendered text sprites, and sprite transforms are not
scene state. They are derived by `koharu-renderer`. The optional rendered
page-level artifact may be persisted as an export/cache result without
duplicating a raster for every text block.

## Reading

`Session` is the committed source of truth:

```rust
let session = koharu_scene::Session::open("book.khr")?;

let project = session.project();
let page = session.page(page_id)?;       // expected O(1)
let (_, element) = session.element(id)?; // expected O(1)
let source = session.read_blob(page.source)?;
```

`Project` and its value types implement Serde for application DTOs. The durable
SQLite encoding is separate and is never an HTTP or frontend protocol.

## Commands

`Commands` is the only mutation path. It is an owned batch tied to a base
revision, so processors and worker tasks can construct batches without holding
a mutable `Session` borrow.

```rust
let mut commands = session.commands();
let page = commands.add_page("001.png", source_bytes)?;
let block = commands.add_text(page, Frame::new(80.0, 120.0, 240.0, 180.0));

commands.push(Command::EditElement {
    page,
    element: block,
    edit: ElementChange::Translation(Some("Hello".into())),
});

let applied = session.apply(commands)?;
```

Image bytes are attached directly by `add_page`, `add_image`, `replace_source`,
`replace_image`, and `set_asset`. The batch hashes them immediately and its
commands refer to `BlobId`; SQLite inserts only referenced attachments and
deduplicates identical bytes.

`Commands::merge` combines processor output built from the same revision. It
rejects competing writes to the same property, deletion versus use of the same
page/element, and duplicate insertions. Independent fields and different
elements merge in deterministic caller order.

### Fluent syntax

`Edit` only appends the same commands:

```rust
let mut edit = session.edit();
let page = edit.add_page("001.png", source_bytes)?;
let text = edit
    .page(page)?
    .add_text(Frame::new(80.0, 120.0, 240.0, 180.0));

edit.page(page)?
    .text(text)?
    .set_source(Some(ocr))
    .set_translation(Some("Hello"))
    .set_opacity(0.8);

edit.page(page)?.set_asset(PageAsset::TextMask, mask_png)?;
edit.commit()?;
```

There is no second mutation implementation behind the fluent API.

## Persistence

One `.khr` file is one SQLite database containing three tables:

```text
project
    schema_version, id, head_revision,
    checkpoint_revision, checkpoint

commits
    revision, parent_revision, changes

blobs
    id, bytes
```

The `project` row stores the latest checkpoint. Each `commits` row stores one
small reversible `StoredBatch`; a change contains both its expected value and
replacement value. This supports safe replay and revert without a second
inverse column. `blobs` is immutable and keyed by the BLAKE3 digest of its
encoded bytes.

There are no page, element, property, role, font, attachment, or blob-reference
tables. Metadata edits append one commit row and update one integer head. Blob
bytes are never copied into commands or checkpoints.

SQLite transactions, WAL, and a short `BEGIN IMMEDIATE` write section provide
durability and writer serialization. See SQLite's
[transaction](https://www.sqlite.org/lang_transaction.html) and
[WAL](https://www.sqlite.org/wal.html) documentation.

### Version-tolerant payloads

Durable Rust values use
[`surrealdb/revision`](https://github.com/surrealdb/revision), not Postcard.
Every persisted struct and enum starts at revision 1. Future fields and variants
must use `start`, `end`, `default_fn`, and `convert_fn` annotations as required.
The crate generates current serializers and historical deserializers, which is
the exact requirement for long-lived checkpoints and command history.

The SQLite `schema_version` remains separate:

- type revisions evolve a BLOB's Rust value;
- `schema_version` evolves tables, columns, or the outer encoding strategy;
- `Revision` is the user's project edit number and is unrelated to either.

Koharu uses Revision's ordinary sequential encoding because it fully decodes a
commit or checkpoint. The indexed/optimized representation would add offsets
and buffering without improving this access pattern. Revision documents its
[wire and compatibility behavior](https://docs.rs/crate/revision/latest).

Before release, CI should run `revision-lock` and retain golden bytes for every
released payload revision. New Koharu versions read older revisions; older
versions are not expected to read projects written by newer versions. Save As
and upgrade flows should therefore use SQLite's backup API before an irreversible
schema migration.

## History, checkpoints, and blobs

- `revert(revisions)` reverses retained changes in newest-first order and
  commits the result as a new revision. Preconditions prevent an old undo from
  overwriting newer edits.
- Automatic checkpoints default to every 1,024 commits. Only checkpointing
  commits serialize the complete project.
- `refresh()` replays new commits incrementally. If required commits were
  pruned, it reloads the current checkpoint.
- `prune_history(keep_from)` checkpoints the head, removes older commits, and
  garbage-collects in one maintenance transaction. Passing `head + 1` removes
  all undo history.
- `gc()` marks blobs referenced by the current project and retained before/after
  history, then removes everything else. It decodes commit metadata but never
  decodes image pixels.
- `backup(path)` uses SQLite's online backup API, so the result contains the
  project, history, and blobs consistently.

## Parallel writes

Parallel processors should read one revision, build independent `Commands`,
and merge them before committing. Multiple `Session`s may also open one file:

1. expensive image validation and change preparation happen before the write
   transaction;
2. the transaction checks that SQLite's head still equals the batch base;
3. a winner appends one commit and advances the head;
4. a loser receives `RevisionConflict`, calls `refresh()`, and recomputes from
   current inputs.

Koharu does not silently rebase model output or use last-writer-wins semantics.
That would make processor results depend on completion order.

## Performance contract

| Operation | Cost |
| --- | --- |
| `Session::page` / `Session::element` | Expected O(1) |
| Scalar text or element edit | Expected O(1) in memory |
| Insert/remove/reorder page | O(number of pages and affected elements) |
| Insert/remove/reorder element | O(elements on that page) |
| Metadata-only commit | O(changes) plus SQLite transaction |
| Attach image or mask | O(encoded bytes) for inspect/hash/write |
| Read blob | O(encoded bytes), lazy |
| Refresh | O(new changes) |
| Open | O(checkpoint size plus later changes) |
| Checkpoint | O(project metadata), excluding blob bytes |
| GC | O(current references plus retained changes plus blob rows) |

Important invariants:

- ordinary commits never clone or serialize the complete project;
- scalar element replacement updates private indexes without rebuilding them;
- page/element vectors are the only ordering authority;
- opening and traversing never read image payloads;
- attached bytes use `Arc<[u8]>` and content-addressed deduplication;
- image dimensions, mask channel count, finite geometry, opacity, text metadata,
  typography, blob digests, and history preconditions are validated;
- disk sessions use WAL while in-memory tests use SQLite's memory journal.

## Intentionally removed

The redesign removes `Scene`, `Node`, `NodeKind`, `Group`, `MaskNode`, generic
parent/child APIs, arena traversal, world-transform helpers, command IDs,
request hashes, generic patches, arbitrary roles, per-text sprites, raw font
predictions, detector names, and renderer-facing caches.

Each was either a general-editor abstraction, duplicated another authority, or
stored data that Koharu can derive. Add a new persistent field only when a real
Koharu workflow cannot be represented without it.
