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

`koharu-storage` only persists the latest opaque complete scene state and blob
bytes. Native operations exist for patching, change reporting, explicit rebase,
and session-local undo; they are not a persistent file format.
Rendering, ML execution, and desktop synchronization remain consumers of the
scene rather than responsibilities of it.

The kernel is purpose-built rather than based on `bevy_ecs`. Koharu needs
ordered parent/child ownership, immutable page-level copy-on-write snapshots,
explicit patch preconditions, revisioned component payloads, and typed relation
indexes; adapting an archetype scheduler would add ownership translation without
removing these responsibilities.

Built-in component registration is declared once in the schema registry. That
declaration drives decoding and a compact component-presence mask used by entity
validation, so adding a built-in component does not require another chain of
repeated per-entity string scans. Cross-component and relation rules remain
ordinary Rust where their conditions need hierarchy or adjacency context.

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
for in-memory undo and exact preconditions needed for explicit rebase.

## Performance invariants

- Ordinary edits never rebuild a project-wide index.
- Patch construction mutates a private scene state and records native ops.
- Component decoding is cached in the immutable component record.
- Component lookup and page-local membership queries use page indexes.
- Subtree observation compares one page epoch; exact component observation
  compares one fingerprint.
- Saving encodes one complete scene state, then storage atomically publishes it.
- Full structural validation happens when loading a scene state; edits perform
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

Scene I/O is asynchronous. `Session::create`, `open`, `memory`, `commit`,
`undo`, and blob reads may cross the filesystem boundary and must never block
the UI executor. Pure snapshot queries, edits, rebases, and typed component
decoding remain synchronous because they are in-memory work.

Each successful commit stores the complete new scene state and retains the
inverse operations in session memory. Closing the session discards undo history;
reopening restores the newest valid state, not an operation log. Undo states
retain their blob scopes so storage collection cannot remove bytes they may
restore.

Assets follow the same boundary: the scene owns their semantic role and blob
reference, while storage owns bytes and leases. Image decoding, layout,
rendering, ML execution, and desktop synchronization remain outside this crate.

Blob reads return `bytes::Bytes`. Callers do not know whether bytes are newly
produced, buffered, or backed by a read-only memory map, and scene does not add
another blob-data or batch wrapper.
