# koharu-storage

`koharu-storage` is Koharu's single-file durable container. It owns redb,
revision ordering, opaque checkpoints and reversible commit envelopes, and
content-addressed blobs. It does not own the scene model or interpret commit
payloads.

## Contract

- One redb database stores metadata, an append-only commit tail, and blobs.
- The caller supplies the initial checkpoint and every later checkpoint.
- Commits contain caller-defined forward and inverse bytes.
- A commit succeeds only when its parent equals the durable database head.
- Refresh is two-phase: storage returns a candidate tail, and advances the
  reader only after the caller successfully applies and accepts it.
- Blob bytes are BLAKE3-addressed, loaded explicitly, and retained by current
  state, retained history, and live snapshot leases.
- Ordinary interactive transactions are atomic but use deferred durability;
  `flush`, checkpoint, maintenance, backup, and orderly close establish the
  durability boundary.

The crate deliberately has no records, components, hierarchy, indexes,
validation, rebase policy, or domain undo model. Those belong to the owner of
the opaque payload—in Koharu, `koharu-scene`.

## Durable layout

```text
meta
  format, document, head, checkpoint revision, checkpoint bytes

commits
  revision -> label, forward bytes, inverse bytes, retained blob IDs

blobs
  BLAKE3 ID -> exact bytes
```

All document semantics are encoded above this layer. This keeps an ordinary
scene edit proportional to its own payload instead of rebuilding or encoding a
generic storage-side project model.
