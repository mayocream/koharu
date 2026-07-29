# koharu-storage

This document is the authoritative design contract for `koharu-storage`.

The crate is a generic, durable component-record store. SQLite remains the
project container and `revision` remains the durable payload codec. Storage is
the source of truth for records, opaque components, blobs, revisions, and
history; domain meaning belongs in crates such as `koharu-scene`.

Existing pre-redesign Rust APIs and file formats are not compatibility
constraints.

## Purpose

`koharu-storage` provides:

- immutable, structurally shared snapshots;
- owned edits producing flat optimistic patches;
- explicit read observations for safe rebasing;
- exact write preconditions and reversible operations;
- content-addressed, lazily read blobs;
- linear SQLite revisions, checkpoints, refresh, undo, and maintenance.

It has no built-in page, hierarchy, relation, node-graph, media, pipeline,
renderer, or collaboration semantics. A domain crate encodes those concepts as
components and declares every record and blob reference outside its opaque
payload.

## Decisions

| Concern | Decision |
| --- | --- |
| Logical model | One document containing stable records with keyed opaque components. |
| Document anchor | Every document owns one permanent root record. |
| Conflict unit | One record lifecycle or one component key. |
| Read state | Cheap immutable `Send + Sync` snapshots over persistent maps. |
| Mutation | An owned editor immediately updates a private persistent state. |
| Patch model | One flat list of reversible operations plus explicit observations. |
| Rebase | Explicit optimistic validation; durable commit never rebases implicitly. |
| Durable history | A linear append-only revision log with periodic checkpoints. |
| Durable store | One SQLite database per document, using WAL and short writes. |
| Blobs | Immutable BLAKE3-addressed bytes stored in SQLite and read lazily. |
| Evolution | `revision` evolves storage envelopes; component owners evolve payloads. |

Graph-shaped application data belongs above this crate. Adding a node editor or
relation model must not require changing the storage engine.

## Core model

```text
Document
|-- permanent root RecordId
|-- RecordId -> Record
|   `-- ComponentKey -> ComponentRecord
`-- BlobId -> bytes
```

The root is an ordinary non-removable record used for document-level
components. Storage assigns it no hierarchy meaning.

Stable identities are UUIDv7 for documents and records, BLAKE3 hashes for blobs
and patches, and a monotonic `u64` revision within one document.

## Components

```rust
pub struct ComponentKey {
    kind: ComponentKind,
    slot: ComponentSlot,
}

pub struct ComponentRecord {
    schema: u32,
    payload: Arc<[u8]>,
    record_refs: Arc<[RecordId]>,
    blob_refs: Arc<[BlobId]>,
}
```

Kinds use a bounded reverse-DNS name. Slots distinguish multiple instances and
normalize an empty value to `default`.

`ComponentRecord::new` bounds payload and reference counts, canonically sorts
and deduplicates references, and computes a fingerprint over schema, payload,
and declared references. Storage does not decode component payloads.

Declared references are authoritative for storage structure:

- record references prevent dangling targets and feed the reverse index;
- blob references keep bytes alive;
- both participate in fingerprints, patches, history, and validation.

The component owner must verify that declared references match the opaque
payload. Unknown kinds and schemas are preserved byte-for-byte.

## State and snapshots

Authoritative state contains persistent records. Reverse record references and
blob counts are derived indexes:

```rust
struct State {
    document: DocumentId,
    revision: Revision,
    root: RecordId,
    records: imbl::HashMap<RecordId, Record>,
    incoming_refs: imbl::HashMap<RecordId, imbl::OrdSet<ComponentAddress>>,
    blob_counts: imbl::HashMap<BlobId, u64>,
}
```

Indexes update incrementally. Open and full validation reconstruct them from
authoritative records. Debug and test builds compare reconstructed indexes
after mutation paths.

`Snapshot` owns an `Arc<State>`, a blob reader, preview attachments, and a blob
lease. Cloning a snapshot is O(1), performs no SQLite work, and takes no global
metadata lock. Record/component reads borrow from immutable state; blob bytes
load only through explicit blob reads.

## Editing and observations

```rust
impl Snapshot {
    pub fn edit(&self) -> Edit;
    pub fn patch(&self, f: impl FnOnce(&mut Edit) -> Result<()>) -> Result<Patch>;
}

impl Edit {
    pub fn insert_record(&mut self) -> Result<RecordId>;
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
    pub fn observe_record(&mut self, record: RecordId) -> Result<()>;
    pub fn observe_component(
        &mut self,
        record: RecordId,
        key: &ComponentKey,
    ) -> Result<()>;
    pub fn attach_blob(&mut self, bytes: impl Into<Arc<[u8]>>) -> BlobId;
    pub fn finish(self) -> Result<Patch>;
}
```

Mutations apply immediately to the edit's private persistent state, giving
early reference errors and read-your-writes behavior.

The editor separately tracks record lifecycles and dirty component addresses.
Finishing compares only those addresses. It serializes only changed before and
after values; untouched sibling components are neither serialized nor copied.
Repeated writes coalesce and writes returning to the base value disappear.

Observations represent values read to produce a different write:

- `observe_component` captures one component fingerprint, including absence;
- `observe_record` captures record existence and a fingerprint of its complete
  component map.

Write operations already carry exact before-values, so a value only needs an
observation when it influenced another key. Domain workflows are responsible
for declaring those inputs. For example, a pipeline stage may observe its page
subtree before writing derived OCR or inpainting results.

## Flat patches and rebasing

```rust
pub struct Patch {
    base: BaseRevision,
    observations: Arc<[Observation]>,
    operations: Arc<[Operation]>,
    attachments: Arc<BTreeMap<BlobId, Arc<[u8]>>>,
    label: Option<Arc<str>>,
}
```

Raw operations and observations are private. A patch is one atomic candidate
commit, not a history DAG or a bag of mergeable segments.

Operations retain exact before and after values:

- insert an empty record;
- remove an empty record;
- replace, insert, or remove one component.

Record creation precedes component installation. Removed records have their
components cleared before lifecycle removal. This makes forward replay and
reverse replay dependency-safe, including reference cycles among records
removed together.

`Snapshot::preview` applies one or more patches in caller order without
advancing the durable revision. Operation preconditions naturally encode
ordered preview dependencies: a descendant patch cannot apply unless earlier
patches established its expected input state.

`Patch::rebase_on(snapshot)` is explicit optimistic rebasing. It:

1. requires the same document;
2. verifies every observation against the target snapshot;
3. verifies every write before-value and record lifecycle;
4. verifies attachment hashes and blob availability;
5. returns a new patch bound to the target revision.

A same-key conflict or changed observed input fails during rebase. An unrelated
change succeeds. `Session::commit` never invokes rebase and always rejects a
stale base revision.

Storage does not merge inside opaque payloads and has no last-writer-wins mode.
Independent writers that must compose should use separate component keys or
commit and explicitly rebase in application-defined order.

## Blobs

Blob IDs are BLAKE3 over exact bytes. New bytes travel as patch attachments and
become durable only if referenced by the committed result. Reads check preview
attachments, a shared byte-bounded cache, then a bounded pool of SQLite reader
connections. Batch reads deduplicate IDs and use bounded SQL chunks.

Every live in-process session and snapshot leases its referenced durable blob
set. Garbage collection additionally retains blobs reachable from current
state and reversible commits.

Process-local leases cannot protect snapshots in another process. Destructive
maintenance therefore requires the application to hold its exclusive
document-maintenance lock.

## SQLite and durable encoding

One document is one database:

```text
meta
    document/root identity, schema version, head/checkpoint revisions,
    checkpoint bytes, checkpoint counters

commits
    revision, parent revision, label, reversible operation payload

commit_blobs
    revision, blob ID retained by reversible history

blobs
    BLAKE3 ID, byte length, bytes
```

File sessions use WAL, foreign keys, a finite busy timeout, and configurable
`NORMAL`/`FULL` synchronous behavior. Only the final persistence section uses
`BEGIN IMMEDIATE`.

Checkpoints and commits are `revision` envelopes. Checkpoint records and
components are canonically sorted. Component payload evolution is independent
of the SQLite schema and storage envelope revision.

One successful non-empty commit advances `Revision` exactly once. An empty
patch verifies the durable head and writes nothing.

Commit work is prepared before the SQLite writer lock:

1. validate document, revision, observations, operations, and attachments;
2. apply operations to a persistent state clone;
3. validate blob availability and structural invariants;
4. encode the reversible commit and optional checkpoint;
5. begin `IMMEDIATE` and compare the durable head;
6. insert new blobs, append the commit, and update metadata;
7. publish the prepared state only after SQLite commit succeeds.

Database failure leaves in-memory state unchanged. Multiple sessions may open
one file; only one expected-head comparison wins.

## Refresh, history, and undo

Normal refresh decodes and applies only commits after the session revision. It
folds those operations directly into one net `ChangeSet`; it does not construct
or compare complete serialized document maps. If required tail history was
pruned, refresh falls back to the latest checkpoint and a full semantic diff.

Undo reads retained commit operations, reverses them newest-first, and commits
the reversal as one new revision. Shared durable history is never silently
rewound. Application code decides which revisions form one user-facing undo
group and whether redo state survives process restart.

Maintenance remains explicit:

- `checkpoint()` records current authoritative state without advancing the
  revision;
- `prune_history()` checkpoints before removing old commits;
- `gc()` preserves current state, retained history, and live in-process leases;
- `backup()` uses SQLite online backup;
- `compact()` checkpoints, prunes, collects blobs, and compacts SQLite.

## Change summaries

`ChangeSet` contains sorted record lifecycle changes, component addresses and
change kinds, and newly inserted blob IDs. It does not duplicate component
payloads. Consumers read current values from the returned snapshot.

Commit summaries fold the patch operations. Normal refresh summaries fold the
replayed commit tail. Only checkpoint fallback performs a whole-state diff.

## Concurrency and performance

- `Session` is synchronous, single-writer, and not `Sync`.
- `Snapshot`, `Patch`, IDs, component records, and blob readers are
  `Send + Sync`.
- Snapshot and patch clones are O(1).
- Replacing one component copies persistent-map paths and serializes only that
  component's changed before/after values.
- Preview cost is proportional to patch operations and persistent updates.
- Ordinary commit cost is proportional to changes, validation scope,
  serialization, and the short SQLite transaction.
- Normal refresh cost is proportional to the replayed tail, not document size.
- Open, checkpoint traversal, and metadata-only operations do not read blob
  bytes.
- Deterministic bytes never depend on hash-map iteration order.

Graphite-style hot-operation streams, Lamport clocks, LWW registers, merge
commits, resurrection, and collaborative history DAGs are intentionally out of
scope. If collaboration becomes a product requirement, it belongs in a
domain-aware synchronization layer rather than this opaque storage core.

## Module layout

```text
src/
  lib.rs          public re-exports
  id.rs           document, record, blob, patch, and revision IDs
  component.rs    keys, opaque records, addresses, fingerprints
  state.rs        persistent records and derived indexes
  snapshot.rs     immutable reads, previews, and blob leases
  edit.rs         owned editor, observations, component-local diff
  patch.rs        flat operations, observations, effects, rebase
  blob.rs         attachments, readers, cache, and leases
  history.rs      durable envelopes, replay, undo, summaries
  session.rs      create/open/commit/refresh/maintenance facade
  storage.rs      SQLite schema and connection setup
  error.rs        structured errors
```

## Verification

Tests and fixtures must cover:

1. O(1) snapshot and patch clones.
2. Component-local diffing without sibling payload serialization.
3. Unknown component preservation through commit, refresh, checkpoint, undo,
   backup, and reopen.
4. Reconstructed and incremental index equality under generated edits.
5. Strict record/blob reference integrity and permanent-root enforcement.
6. Explicit rebase success for unrelated writes and failure for same writes.
7. Rebase failure when a declared component or record observation changes.
8. Ordered previews where descendants require state established by earlier
   patches.
9. Tail-folded refresh summaries with checkpoint fallback coverage.
10. Blob attachment hashing, lazy reads, pins, retained-history GC, and backup.
11. Concurrent-session head conflicts with no silent overwrite.
12. Golden checkpoint and commit bytes for every released envelope revision.
13. Bounded errors for corrupt counts, hashes, chains, and truncated payloads.
