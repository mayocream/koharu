# koharu-storage

`koharu-storage` owns one filesystem-native project format: the latest complete
opaque state plus immutable content-addressed blobs. It is document persistence,
not version control. Undo, redo, scene semantics, rendering, and autosave policy
belong above this crate.

## Format

```text
project.khrproj/
|-- state-a.khr
|-- state-b.khr
|-- blobs/
|   `-- ab/
|       `-- <BLAKE3 blob id>
`-- project.lock
```

The two state files are alternating complete snapshots. Each contains a magic
value, format version, document ID, monotonic revision, complete referenced-blob
set, opaque payload, and BLAKE3 checksum. On open, storage validates both slots
and selects the newest complete state whose blobs exist. If that slot is corrupt
or incomplete, the previous valid slot is used.

There is no RocksDB database, operation log, commit graph, `HEAD`, checkpoint
policy, storage snapshot wrapper, or Koharu-managed temporary directory.

Blob files are immutable and named by the BLAKE3 hash of their exact bytes.
Reusing an ID performs no write. Large durable blobs are returned as
owner-backed `bytes::Bytes` over a read-only `memmap2::Mmap`; small blobs and
newly produced bytes use the same `Bytes` type without exposing where their
storage came from.

## Ownership

`Session` owns the project path, single-writer interprocess lock, serialized
publisher, and current durable head. Clones share those owners.

`State` is one immutable opaque payload and its complete `Blobs` scope. `Blobs`
owns both reachability and byte lifetimes. A mapped byte value retains its lease,
so garbage collection cannot delete its file while it can still be read.

`koharu-scene` owns payload encoding, entity invariants, referenced-blob
enumeration, and in-memory undo. The application owns dirty state and autosave
scheduling. Neither layer implements filesystem durability.

## Public API

The public API passes one complete state. Blob bytes are not a parallel argument
and there is no public blob lease or batch wrapper:

```rust,ignore
let session = Session::open(path).await?;
let current = session.load().await?;

let image = Bytes::from(encoded_image);
let image_id = BlobId::for_bytes(&image);
let next = current.update(
    current.revision().next().unwrap(),
    encoded_scene,
    referenced_blob_ids,
    [(image_id, image)],
)?;

let durable = session.save(&next).await?;
let bytes = durable.blobs().get(image_id).await?;
session.collect_garbage().await?;
```

`State::update` derives a new complete state from an existing one. It retains
still-referenced pending bytes, validates newly available bytes against their
IDs, and rejects bytes that the state does not reference. `Session::save`
publishes missing blobs first and then the state, returning a canonical state
whose blob scope is entirely durable.

There is deliberately no generic backend trait, repository, manager,
transaction, `Save`, `Blob`, `BlobBatch`, `BlobLease`, or `Snapshot` facade.

## Publication and recovery

Only one save per open project publishes at a time:

1. Validate document ownership and require a revision newer than the durable
   head.
2. Publish each missing referenced blob with `tempfile::NamedTempFile` created
   beside its destination, flush it, and atomically persist it under its hash.
3. Encode the complete state and checksum.
4. Publish it through another destination-local `NamedTempFile` into the
   inactive state slot.
5. Advance the in-memory durable head only after publication succeeds.

Koharu does not create or clean a temporary folder. An unpublished tempfile is
never considered state. The active slot is left untouched while the inactive
slot is built, so a failed save cannot destroy the previous loadable state.

All filesystem work, hashing, and mapping that may block runs on Tokio blocking
workers. Async here protects the UI executor; it does not imply concurrent
writes to one project.

## Garbage collection

Collection marks blob IDs referenced by both valid on-disk state slots and all
live `Blobs` scopes, including mapped readers and scene undo history. It then
removes unmarked immutable blob files. Collection is explicit and never part of
the save critical path.

## Performance rules

- State loading is bounded and validates lengths before allocation.
- A state payload is decoded only by its owning layer.
- Blob presence checks do not read bytes.
- Blob IDs are deduplicated and sorted in state files.
- Large reads are lazy OS-backed mappings with no second full-size byte buffer.
- Saves skip already published blobs.
- The writer lock is never held by scene mutation code.

The initial format writes one complete scene payload per save. Before adding
page sharding or another indirection, benchmarks must show that scene encoding
or state-file writes dominate representative saves. Content-addressed immutable
pages would still be a snapshot format, not a reason to reintroduce commits.

## Required verification

Focused tests and benchmarks cover create/open/save, newest-slot selection,
corrupt and missing-blob fallback, stale-save rejection, lock lifetime, blob
hash validation, mmap reads, deduplication, live-lease retention, garbage
collection, hostile state bounds, and representative open/save/read costs.

The RocksDB format is intentionally not accepted through compatibility code. A
one-shot migration utility, if real user data requires one, is a separate
explicit project.
