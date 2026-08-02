# koharu-scene

`koharu-scene` is Koharu's canonical in-memory project model. Its public design
has three ownership layers:

- The scene kernel owns stable identity, ordered hierarchy, revisioned
  component records, relation adjacency, indexes, observations, patches, and
  undo. It does not decide what a document entity means.
- The document schema owns typed roles, typed relations, valid component
  combinations, and analysis/content/presentation invariants.
- The editor facade owns resolved views and intent-level creation operations,
  so consumers do not have to reconstruct a text layer from raw components and
  relation strings.

`koharu-storage` only persists opaque checkpoints, operations, and blob bytes.
Rendering, ML execution, and desktop synchronization remain consumers of the
scene rather than responsibilities of it.

## Runtime model

Each page is an independent arena:

```text
Project
  ordered page IDs
  project components
  relations and endpoint indexes

Page arena
  slotmap local entity keys
  stable external EntityId -> local key
  parent and ordered child keys
  compact sorted component sets
  per-component membership indexes
  mutation epoch
```

Stable IDs cross API and persistence boundaries. Slotmap keys exist only inside
one loaded page and make hierarchy traversal and local mutation cache-friendly.
Persistent maps and `Arc::make_mut` share untouched pages between immutable
snapshots. Editing one page clones that page; moving a subtree between pages
clones only the source and destination arenas.

Hierarchy is native state, not a synthetic component. Scene operations include
page and entity insertion, removal and movement, component replacement, and
relation lifecycle changes. Every operation carries the exact inverse needed
for undo and exact preconditions needed for explicit rebase.

## Performance invariants

- Ordinary edits never rebuild a project-wide index.
- Patch construction mutates a private scene state and records native ops.
- Component decoding is cached in the immutable component record.
- Component lookup and page-local membership queries use page indexes.
- Subtree observation compares one page epoch; exact component observation
  compares one fingerprint.
- Commit encodes only the native operations and optional threshold checkpoint.
- Full structural validation happens when loading a checkpoint; edits perform
  incremental validation for the state they touch.
- Derived renderer, canvas, and UI state consumes explicit hierarchy,
  component, entity, and relation changes.

## Components and snapshots

Entities remain open-ended collections of revisioned typed components. Adding a
component does not change a central entity enum. `Page`, `Relation`, and entity
origin use dedicated structural APIs; normal values use the generic typed
component API.

An owner has at most one component of a Rust type. There are no named component
slots and no implicit `default` value. A concept that needs multiplicity must
model it explicitly as entities and typed relations, or as a collection owned
by one component. This keeps identity and ownership visible in the schema.

Text analysis, content, and presentation are separate entities:

```text
TextLayout + Typography                         presentation entity
  -- presents --> TextContent + SourceText + Translation    content entity
                      -- recognized-from --> Region(text) + Geometry + OcrAnalysis
  -- fits-to ---------------------------> Region(text or bubble) + Geometry
Region(text) -- inside -----------------> Region(bubble)
```

Detection and OCR geometry describe the source artwork and never double as an
editable layer. A text layer with its own `Geometry` has a manual presentation
frame. Without one, its frame is derived from `fits-to`; renderer layout bounds
remain transient output. This lets source regions, semantic text, and visual
typesetting change independently while retaining explicit provenance.

Snapshots are immutable and cheap to clone. A patch is bound to a project and
base revision. Stale independent work must call explicit `rebase_on`; commits
never silently merge or apply last-writer-wins behavior.

Assets follow the same boundary: the scene owns their semantic role and blob
reference, while storage owns bytes and leases. Image decoding, layout,
rendering, ML execution, and desktop synchronization remain outside this crate.
