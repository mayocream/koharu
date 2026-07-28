# koharu-storage

This document is the authoritative design contract for `koharu-storage`.
The implementation is the greenfield component-record engine described here;
it is not the former scene model under a new package name.

This is a greenfield redesign. Preserve SQLite as the project container and
the `revision` crate for versioned durable payloads. Existing Rust APIs,
domain models, SQLite tables, serialized values, and project files are not
compatibility constraints.

## Purpose

`koharu-storage` is a durable, versioned component-record store. It provides
immutable snapshots, owned edits, dependency-aware patches, content-addressed
blobs, revisions, history, and atomic SQLite commits.

It knows how records and components are stored, referenced, merged, validated
structurally, and recovered. It does not know what those records mean.

In particular, storage has no concepts named page, scene entity, text, image,
geometry, translation, typography, mask, region, model, or renderer. Those
belong in `koharu-scene` and other domain crates.

## Decisions

| Concern | Decision |
| --- | --- |
| Logical model | One immutable document containing stable records with keyed opaque components. |
| Document anchor | Every document owns one permanent root record for document-level components. |
| Extensibility | Storage preserves opaque component payloads and declared references without decoding them. |
| Conflict unit | One record lifecycle or one component key. |
| Read state | Cheap, immutable, `Send + Sync` snapshots over structurally shared state. |
| Mutation | An owned editor produces an immutable patch; it never borrows a session. |
| Branches | Patch segments record preview ancestry so descendants compose and siblings conflict deterministically. |
| In-memory collections | Private `imbl` maps, ordered maps, sets, and vectors. |
| Durable store | One SQLite database per document, using WAL and short write transactions. |
| Durable encoding | `revision` for checkpoints and reversible commit payloads. |
| Blobs | Immutable BLAKE3-addressed bytes with lazy reads and snapshot-safe garbage collection. |
| References | Components explicitly declare record and blob references outside their opaque payload. |
| Derived indexes | Reverse-record references and blob counts are rebuilt from authoritative records on open. |

## Responsibilities

`koharu-storage` owns:

- document, record, blob, patch, and revision identities;
- immutable records and opaque component envelopes;
- structural sharing and lock-free snapshot reads;
- explicit record and blob reference integrity;
- owned edits, previews, patch ancestry, merge conflicts, and preconditions;
- content-addressed blob insertion, lazy reading, pinning, and collection;
- SQLite schema, checkpoints, commit replay, refresh, undo, pruning, backup,
  and compaction;
- structural change summaries and bounded corruption handling.

It does not own:

- ordered scene hierarchy, parent indexes, reparenting, or subtree semantics;
- first-class graph edges or relation kinds;
- typed component codecs or application schema registration;
- semantic validation of opaque payload bytes;
- model provenance, pipeline phases, scheduling, or cancellation;
- image decoding, media metadata, rendering, or export;
- frontend serialization and protocol DTOs;
- configuration, model weights, caches unrelated to document blobs, or general
  application storage.

The crate is synchronous and has no async-runtime dependency. `Session` is a
single-writer storage handle. `Snapshot`, `Patch`, component records, and blob
readers can cross threads.

## Core model

```text
Document
|-- permanent root RecordId
|-- RecordId -> Record
|   `-- ComponentKey -> ComponentRecord
`-- BlobId -> bytes
```

The root is an ordinary record created with the document and never removed.
It provides a stable place for domain-level metadata and root components.
Storage gives it no hierarchy or scene meaning.

There are no built-in child collections or relation records. A domain layer
can encode ordered children or relation endpoints in components and declare
their referenced record IDs. This keeps storage useful without baking one
document topology into the durable engine.

## Stable identities

```rust
pub struct DocumentId(Uuid);
pub struct RecordId(Uuid);
pub struct PatchId([u8; 32]);
pub struct BlobId([u8; 32]);
pub struct Revision(u64);
```

UUID-based IDs never encode collection indexes, process addresses, record
kinds, parents, or payload schemas. IDs are not reused. Inserting an existing
record ID is an error, even if the previous value appears identical.

`BlobId` is BLAKE3 over the exact byte sequence. `PatchId` is BLAKE3 over a
canonical patch segment representation.

`Revision` is monotonic within one document. It advances once per successful
non-empty durable commit, not once per patch segment or pipeline node.

## Component addressing

A record contains zero or more components selected by a stable key:

```rust
pub struct ComponentKind(Arc<str>);
pub struct ComponentSlot(Arc<str>);

pub struct ComponentKey {
    kind: ComponentKind,
    slot: ComponentSlot,
}
```

Kinds use a bounded validated reverse-DNS namespace, for example
`dev.koharu.scene.children`. A slot distinguishes multiple instances of one
kind, such as locale-specific values. Empty slots normalize to `default`.

Storage compares keys but never interprets them. Kind and slot values are
permanent durable identifiers once released by their owning domain.

## Opaque component records

```rust
pub struct ComponentRecord {
    schema: u32,
    payload: Arc<[u8]>,
    record_refs: Arc<[RecordId]>,
    blob_refs: Arc<[BlobId]>,
}
```

Fields are private. `ComponentRecord::new` is the only constructor and:

- enforces bounded payload and reference counts;
- sorts and deduplicates both reference arrays;
- rejects malformed IDs and duplicate references;
- calculates a canonical fingerprint over schema, payload, and references.

`schema` belongs to the component owner. Storage persists and fingerprints it
but never migrates or validates the payload schema.

Explicit references serve three structural purposes:

1. `record_refs` prevent dangling references and support reverse lookup.
2. `blob_refs` keep required blobs alive without decoding payloads.
3. Both participate in fingerprints, patches, history, and deterministic
   validation.

The component owner must ensure the declared references match its payload.
Storage cannot prove that an opaque byte sequence mentions no undeclared ID.
Koharu-owned codecs test this invariant with golden fixtures and round trips.

Unknown kinds and unknown schema numbers are preserved byte-for-byte. Editing
another component never decodes, rewrites, or normalizes them.

## Authoritative state and derived indexes

```rust
struct State {
    document: DocumentId,
    revision: Revision,
    root: RecordId,

    // Authoritative and checkpointed.
    records: imbl::HashMap<RecordId, Record>,

    // Derived and never checkpointed.
    incoming_refs: imbl::HashMap<RecordId, imbl::OrdSet<ComponentAddress>>,
    blob_counts: imbl::HashMap<BlobId, u64>,
}

struct Record {
    components: imbl::OrdMap<ComponentKey, ComponentRecord>,
}

pub struct ComponentAddress {
    pub record: RecordId,
    pub key: ComponentKey,
}
```

The permanent root must exist. Every declared `record_ref` must resolve to a
record in the same document. Every referenced blob must be durable or supplied
by the patch being previewed or committed.

`imbl` is private. Public APIs and durable payloads never expose its types.
Checkpoints encode records sorted by `RecordId` and components sorted by key,
so collection implementation and hash iteration cannot affect bytes.

Normal operations update derived indexes incrementally. Reliability does not
depend only on that logic:

- open rebuilds all derived indexes from authoritative records;
- debug builds and tests independently reconstruct and compare indexes after
  edits, previews, commits, replay, and undo;
- a full validation path ignores existing derived indexes;
- new derived indexes can be added without a file-format migration.

This consistency net is mandatory. An index mismatch is a storage bug or file
corruption, never a recoverable alternate state.

## Snapshot API

```rust
#[derive(Clone)]
pub struct Snapshot {
    state: Arc<State>,
    blobs: BlobReader,
    lineage: PatchLineage,
}

impl Snapshot {
    pub fn document_id(&self) -> DocumentId;
    pub fn revision(&self) -> Revision;
    pub fn root(&self) -> RecordId;

    pub fn record(&self, id: RecordId) -> Result<RecordRef<'_>>;
    pub fn contains_record(&self, id: RecordId) -> bool;
    pub fn records(&self) -> impl Iterator<Item = RecordRef<'_>>;

    pub fn component(
        &self,
        record: RecordId,
        key: &ComponentKey,
    ) -> Result<Option<&ComponentRecord>>;

    pub fn incoming_references(
        &self,
        record: RecordId,
    ) -> Result<impl Iterator<Item = &ComponentAddress>>;

    pub fn has_blob(&self, id: BlobId) -> bool;
    pub fn read_blob(&self, id: BlobId) -> Result<Arc<[u8]>>;
    pub fn read_blobs(
        &self,
        ids: impl IntoIterator<Item = BlobId>,
    ) -> Result<BlobBatch>;
}
```

Snapshot clone is O(1), touches no SQLite connection, and acquires no metadata
lock. Component and record reads return borrowed opaque data. Blob bytes load
lazily.

`records()` has explicitly unspecified order. Domain code requiring semantic
order must obtain it from a domain component. Checkpoints and change summaries
sort independently of this iterator.

Storage exposes no typed `Component` trait. Encoding, decoding, migration, and
semantic queries belong in `koharu-scene` or another domain crate.

## Owned editing

```rust
impl Snapshot {
    pub fn edit(&self) -> Edit;
    pub fn patch(
        &self,
        f: impl FnOnce(&mut Edit) -> Result<()>,
    ) -> Result<Patch>;
}

impl Edit {
    pub fn insert_record(&mut self) -> Result<RecordId>;
    pub fn insert_record_with_id(&mut self, id: RecordId) -> Result<()>;
    pub fn remove_record(&mut self, id: RecordId) -> Result<()>;

    pub fn set_component(
        &mut self,
        record: RecordId,
        key: ComponentKey,
        value: ComponentRecord,
    ) -> Result<()>;
    pub fn remove_component(
        &mut self,
        record: RecordId,
        key: &ComponentKey,
    ) -> Result<()>;

    pub fn attach_blob(&mut self, bytes: impl Into<Arc<[u8]>>) -> BlobId;
    pub fn view(&self) -> EditView<'_>;
    pub fn finish(self) -> Result<Patch>;
}
```

`Edit` owns a cheap persistent clone of its base state and applies operations
immediately. A record must exist before a component can reference it. A record
cannot be removed while another component references it. Domain helpers must
therefore update references before removing their targets.

Applying immediately gives early errors and lets later operations observe
records created earlier in the same edit.

The editor:

- tracks dirty record IDs so finishing an edit diffs only records touched by
  that edit rather than scanning the document;
- coalesces repeated writes to one component key;
- removes writes returning to the observed base fingerprint;
- deduplicates attachments by `BlobId`;
- records before-value and lifecycle observations;
- updates reverse references and blob counts with the same mutation function;
- drops attachments not referenced by the final edit state;
- emits deterministic dependency-safe operations.

Raw operation enums remain private. Preview, commit replay, undo, and editing
all invoke the same operation application implementation.

## Dependency-aware patches

```rust
pub struct Patch {
    base: BaseRevision,
    segments: Arc<[Arc<PatchSegment>]>,
}

pub struct BaseRevision {
    pub document: DocumentId,
    pub revision: Revision,
}

struct PatchSegment {
    id: PatchId,
    requires: BTreeSet<PatchId>,
    operations: Arc<[Operation]>,
    attachments: BTreeMap<BlobId, Arc<[u8]>>,
}
```

One finished `Edit` creates one segment. Its requirements are the segment IDs
in the base snapshot's preview lineage. A durable session snapshot has empty
lineage.

```rust
impl Snapshot {
    pub fn preview<'a>(
        &self,
        patches: impl IntoIterator<Item = &'a Patch>,
    ) -> Result<Snapshot>;
}

impl Patch {
    pub fn merge<'a>(
        patches: impl IntoIterator<Item = &'a Patch>,
    ) -> Result<Patch>;

    pub fn fingerprint(&self) -> PatchId;
}
```

Preview applies patches only in memory and does not advance the durable
revision. Requirements must already be present. Reapplying an identical
segment is idempotent.

Merge requires one document and base revision, deduplicates identical segments
and attachments, validates ancestry, and preserves the caller's canonical
dependency order. `fingerprint` covers the base, ordered segment identities,
and commit label, allowing a scheduler to prove completion-order-independent
canonical output. Storage also exposes a documentation-hidden effect summary
used by trusted domain wrappers such as `koharu-scene`; orchestration code does
not inspect raw operations or footprints.

### Conflicts

Storage has only two write-key classes:

```text
RecordLife(RecordId)
Component(RecordId, ComponentKey)
```

Operations retain their exact before and after values. Merge derives write and
record-access sets from those operations; apply checks prior values and
referenced-target existence against the supplied snapshot.

The rules are:

- ancestor and descendant segments are sequential;
- unrelated siblings writing the same key conflict;
- siblings writing different components on one record compose;
- siblings inserting different records compose;
- removal conflicts with sibling access to that record or a sibling component
  introducing an inbound reference;
- duplicate record IDs, missing requirements, dangling references, failed
  fingerprints, and mismatched blob hashes are always errors;
- identical segment IDs are included once.

Storage never merges inside opaque payloads. If independent writers must
compose, the domain schema must place their values in separate component keys.

## Blobs

Storage owns bytes, not media semantics:

```rust
pub struct BlobAttachment {
    id: BlobId,
    bytes: Arc<[u8]>,
}
```

There is no media type, filename, image size, color model, or decoded value in
the blob table. Such metadata belongs in a component referencing the blob.

Reads check preview attachments, then a shared byte-bounded cache, then a
bounded pool of read-only SQLite connections. Batch reads deduplicate IDs.
No storage lock covers decoding or application work.

Every live state owns a pin representing its referenced durable blob set.
Snapshots share that pin; preview attachments are held by their overlay.
Garbage collection marks blobs reachable from:

1. the current state;
2. every live session and snapshot state pin;
3. retained reversible commits;
4. preview overlays participating in the maintenance process.

Only unmarked blobs may be deleted. Process-local pins cannot protect another
process, so destructive maintenance requires the application's exclusive
document-maintenance lock.

## SQLite format

One document is one SQLite database:

```text
meta
    singleton, schema_version, document_id, root_record_id,
    head_revision, checkpoint_revision, checkpoint,
    commits_since_checkpoint, bytes_since_checkpoint

commits
    revision, parent_revision, label, payload

commit_blobs
    revision, blob_id

blobs
    id, byte_len, bytes
```

`meta` has one row. `commits` is a linear append-only revision log.
`commit_blobs` records the union of before and after references needed by each
retained reversible commit. `blobs` is immutable.

Records and components are not normalized into SQL tables. Normal reads consume
coherent immutable snapshots, while changes need reversible atomic commits. A
compact checkpoint plus granular commit tail matches this access pattern and
keeps component evolution outside SQLite.

Configuration is explicit:

- file sessions use `journal_mode = WAL`;
- `foreign_keys = ON`;
- busy timeout is finite and configurable;
- `synchronous = NORMAL` is the default and can be strengthened;
- statements for head, commits, and blobs are cached;
- only the final short write section uses `BEGIN IMMEDIATE`.

The schema begins at version 1. There is no migration from the former
`koharu-scene` database format.

## Durable encoding with revision

```rust
#[revisioned(revision = 1)]
struct Checkpoint {
    document: DocumentId,
    root: RecordId,
    records: Vec<StoredRecord>, // sorted by RecordId
}

#[revisioned(revision = 1)]
struct StoredRecord {
    id: RecordId,
    components: Vec<StoredComponent>, // sorted by ComponentKey
}

#[revisioned(revision = 1)]
struct StoredCommit {
    label: Option<String>,
    operations: Vec<Operation>,
}
```

Component payloads remain opaque bytes inside these revisioned envelopes.
Storage payload evolution and component payload evolution are independent.

Operations are reversible:

- record insert/remove stores the complete small record and expected lifecycle;
- component replacement stores only before and after component records;
- blob bytes remain in `blobs`, not in commit payloads.

Replay and undo verify every expected before side. A mismatch is corruption or
a programming error, not a reason to use last-writer-wins.

Four version concepts remain distinct:

- SQLite `schema_version` changes tables or outer storage strategy;
- `revision` annotations evolve checkpoints and commit payloads;
- component `schema` is opaque domain-owned metadata;
- `Revision` is the document's monotonic edit number.

CI keeps golden checkpoint and commit bytes for every released storage payload
revision. New storage versions must open and replay all released fixtures.

## Session and atomic commit

```rust
pub struct Session {
    // writer connection, current Arc<State>, shared BlobStore
}

pub struct CommitResult {
    pub revision: Revision,
    pub changes: ChangeSet,
    pub snapshot: Snapshot,
}

impl Session {
    pub fn create(path: impl AsRef<Path>) -> Result<Self>;
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;
    pub fn memory() -> Result<Self>;
    pub fn snapshot(&self) -> Snapshot;
    pub fn commit(&mut self, patch: Patch) -> Result<CommitResult>;
}
```

Commit performs expensive work before SQLite's writer lock:

1. Verify document, base revision, requirements, conflicts, and attachments.
2. Apply operations to a persistent clone of current state.
3. Validate record references, blob references, and derived indexes.
4. Drop unused attachments and calculate the exact blob delta.
5. Serialize the commit and optional checkpoint.
6. Begin `IMMEDIATE` and verify SQLite's head still equals the patch base.
7. Insert referenced new blobs, append the commit and `commit_blobs`, update
   `meta`, and commit SQLite.
8. Publish the prepared `Arc<State>` only after SQLite succeeds.
9. Return the prepared snapshot and deterministic change summary.

Database failure leaves in-memory state unchanged. Competing writers receive
`RevisionConflict { expected, actual }`. Storage never silently rebases.

An empty patch verifies the durable head, writes nothing, and does not advance
the revision.

## Refresh, history, and maintenance

`Session::refresh()` applies only commits after the session's current revision.
If required history was pruned, it reloads the latest checkpoint and tail.
Existing snapshots remain immutable.

Undo creates a new commit:

```rust
pub fn undo(&mut self, revision: Revision) -> Result<CommitResult>;
pub fn undo_many(
    &mut self,
    revisions: impl IntoIterator<Item = Revision>,
) -> Result<CommitResult>;
```

Revisions are reversed newest-first, preconditions are checked, and all
reversals become one atomic new revision.

Maintenance is explicit:

- `checkpoint()` writes a current checkpoint without advancing `Revision`;
- `prune_history(keep_from)` checkpoints before removing older commits;
- `gc()` respects current state, retained history, and live snapshot pins;
- `backup(path)` uses SQLite's online backup API;
- `compact()` performs requested checkpoint, prune, GC, and SQLite compaction.

Checkpoints trigger on both commit count and accumulated serialized bytes.
Serialization occurs before the write transaction.

## Change summaries

```rust
pub struct ChangeSet {
    pub from: Revision,
    pub to: Revision,
    pub records: Vec<RecordChange>,
    pub components: Vec<ComponentChange>,
    pub blobs_added: Vec<BlobId>,
}

pub enum RecordChange {
    Inserted(RecordId),
    Removed(RecordId),
}

pub struct ComponentChange {
    pub address: ComponentAddress,
    pub kind: ValueChangeKind,
}
```

Summaries contain identities and change kinds, not payload copies. Consumers
read values from `CommitResult::snapshot`. Entries are sorted by record ID and
component key.

## Validation and limits

Storage validates only what it owns:

- IDs, key syntax, sizes, counts, hashes, and canonical reference arrays;
- root existence and non-removability;
- component-address uniqueness;
- record-reference existence and reverse-index equality;
- blob-reference existence and count-index equality;
- patch ancestry, write conflicts, and operation preconditions;
- checkpoint, commit-chain, and SQLite-head coherence.

It does not decode payloads or validate domain relationships hidden in them.

Finite hard structural limits cover records, components per record, component
payload bytes, reference counts, key lengths, patch operations, commit bytes,
and checkpoint bytes. Runtime options additionally configure blob size, cache
size, reader concurrency, and checkpoint thresholds. Counts and arithmetic are
checked before publication. Corrupt files return structured errors instead of
publishing partially validated state.

No public mutation API bypasses structural validation. The storage core needs
no `unsafe`.

## Concurrency contract

- `Session` is synchronous, single-writer, and not `Sync`.
- `Snapshot`, `Patch`, `BlobReader`, IDs, and component records are
  `Send + Sync`.
- Snapshot metadata reads are lock-free.
- Snapshot clone and handoff do not touch SQLite.
- Previews can run concurrently on independent branches.
- Payloads and blob bytes move through `Arc` handles.
- Blob cache and reader-pool locks do not cover caller work.
- Multiple sessions may open one file; one head comparison wins and stale
  writers get an explicit conflict.
- Open and refresh use coherent SQLite read transactions.
- Old snapshots remain valid and keep their referenced blobs pinned.

`ArcSwap` is unnecessary while callers pass explicit snapshots. It may be used
by an application-owned latest-snapshot publisher without changing storage.

## Performance contract

| Operation | Target cost |
| --- | --- |
| Clone snapshot or patch | O(1) |
| Record lookup | Persistent-map path; no global scan |
| Component lookup | Record-map path plus component-map path |
| Replace one component | Copies persistent map paths and the new record envelope only |
| Insert/remove record | Copies affected persistent map and derived-index paths |
| Reverse-reference query | Derived-index lookup plus incoming addresses |
| Preview | O(operations times persistent update cost) |
| Merge | O(segments + requirements + declared accesses + attachments) |
| Metadata commit | O(changes + validation scope + serialization + short SQLite transaction) |
| Open | O(checkpoint records + commit tail), without blob payload reads |
| Batch blob read | O(unique IDs + returned bytes) |
| GC | O(live references + retained commit references + blob rows) |

Required invariants:

- ordinary edits and previews never clone the complete document;
- replacing one component never serializes its sibling components;
- deterministic output never depends on hash-map iteration order;
- open, checkpoint, merge, and metadata traversal never read blob bytes;
- preview attachments are readable before durable commit;
- equal blobs are stored once;
- no-op writes disappear before persistence;
- branch completion order cannot affect a canonically merged patch;
- one successful higher-level operation can commit many patch segments as one
  storage revision.

Edits and previews maintain reverse-reference and blob-count indexes
incrementally in release builds. Full independent index reconstruction remains
mandatory on open/replay and runs after edit/preview application in debug and
test builds. Durable change summaries are folded from canonical patch
operations, so an ordinary commit does not diff every record in the document.

The `storage` Criterion benchmark uses 2,048 records and covers snapshot clone,
component lookup, dirty-record patch construction, preview, and a commit of
independent segments. Larger fixture benchmarks additionally report
allocations, cross-record references, large blobs, deep ancestry, and
concurrent sibling patches.

## Module layout

```text
src/
  lib.rs          public re-exports
  id.rs           DocumentId, RecordId, BlobId, PatchId, Revision
  component.rs    keys, records, addresses, fingerprints
  state.rs        persistent records, derived indexes, full validation
  snapshot.rs     immutable reads, lineage, live blob pins
  edit.rs         owned record/component editor
  patch.rs        operations, segments, observations, conflicts, merge
  blob.rs         attachments, lazy readers, cache, pins
  history.rs      revisioned checkpoints, commits, replay, undo, summaries
  session.rs      create/open/commit/refresh/maintenance facade
  storage.rs      SQLite schema, metadata, and connection setup
  error.rs        structured errors
```

There are no scene model modules, graph arenas, typed component codecs, image
types, public operation enums, borrowed-session edits, or alternate mutation
paths.

## Dependency shape

Direct dependencies remain focused:

- `rusqlite` for the only durable store;
- `revision` for storage checkpoint and commit evolution;
- `blake3` for blobs, fingerprints, and patch IDs;
- `imbl` for private persistent collections;
- `uuid` for document and record identities;
- a small synchronization and byte-bounded cache implementation for blob
  readers and live pins.

Do not add `koharu-scene`, `petgraph`, `image`, an async runtime, an ORM, an ECS
framework, a renderer, frontend schema libraries, or a generic plugin system.

## Removed rather than migrated

The storage rewrite does not preserve:

- the former page/element/text/image/region/style model;
- ordered scene hierarchy or first-class relations;
- scene-specific IDs, origins, producers, assets, masks, or geometry;
- typed component APIs;
- old command and edit APIs;
- old checkpoint, commit, blob, or SQLite schemas;
- old project files;
- worker or shared-memory transfer support;
- Serde/Specta frontend shapes.

SQLite and `revision` are retained technologies, not compatibility promises to
the former scene implementation.

## Verification

The redesign is complete when tests and benchmarks prove:

1. Snapshot and patch clones do not copy record collections, component bytes,
   or blob bytes.
2. Replacing one component structurally shares every untouched record and
   sibling component.
3. Unknown component kinds and schemas survive open, unrelated edit, commit,
   checkpoint, replay, undo, and backup byte-for-byte.
4. Rebuilt reverse-reference and blob-count indexes always equal incrementally
   maintained indexes under generated operation sequences.
5. Missing records, dangling references, duplicate IDs, broken hashes, and
   invalid roots are rejected before a snapshot is published.
6. Independent sibling patches can write different components on one record;
   same-component and lifecycle conflicts are deterministic.
7. An ancestor can insert a record and a descendant can reference it; the
   descendant cannot preview or commit without the ancestor.
8. Every permutation of sibling completion produces the same result when
   segments are supplied in canonical dependency order.
9. Preview attachments are readable before SQLite commit.
10. A successful merged patch creates one revision; validation failure,
    cancellation by the caller, stale base, merge conflict, and SQLite failure
    create none.
11. Concurrent sessions cannot overwrite one another and refresh applies only
    the required tail.
12. Undo, replay, checkpoint, reopen, prune, GC, and backup preserve exact
    authoritative records and opaque payload bytes.
13. GC cannot remove a blob reachable from current state, retained history, an
    old live snapshot, or a preview overlay.
14. Corrupt counts, excessive sizes, truncated revision payloads, and invalid
    SQLite chains return bounded structured errors.
15. Large-document benchmarks confirm component-local copying, lazy blob
    access, bounded caches, and no blob reads on metadata-only paths.
