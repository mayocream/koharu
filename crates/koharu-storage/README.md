# koharu-storage

`koharu-storage` owns the durable, model-agnostic contents of one `.khrproj`
directory. The directory is a RocksDB database; there is no manifest or JSON
sidecar.

The database has four column families:

- `document`: the document identity, current revision, and checkpoint
- `history`: ordered opaque forward/inverse commits
- `blob-index`: content IDs and byte lengths used for fast validation and GC
- `blobs`: BLAKE3-addressed payloads stored through integrated BlobDB

Scene structure, validation, conflict rebasing, indexes, and undo semantics
belong to `koharu-scene`. Storage only enforces revision ordering, atomic
commits, content addressing, and durability boundaries.

Normal edits are one WAL-backed RocksDB `WriteBatch` across all affected column
families. Explicit flush and checkpoint operations synchronize the WAL. BlobDB
garbage collection is enabled and project compaction reclaims deleted payloads.
