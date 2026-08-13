# koharu

`koharu` is Koharu's native Tauri application. It owns startup, diagnostics,
commands, application state, pipeline policy, and desktop integration.

## Application boundary

```text
React
  | direct Tauri commands
  | typed IPC channels
  v
koharu
  | Tauri-managed project, canvas, pipeline, jobs, and channel state
  +-> koharu-scene
  +-> koharu-desktop -> koharu-canvas
  +-> koharu-renderer -> raster / koharu-psd
```

Every operation has a named Tauri command. Commands that mutate a project take
its id and current revision directly. The frontend serializes those mutations
and uses the returned revision for the next call.

Native updates do not share an event envelope. `connect` binds independent
typed channels for project snapshots, canvas state, jobs, downloads,
preferences, resource telemetry, and cleanup reports. Tauri state is the only
application state container.

Thumbnails are read with `get_thumbnail`; the frontend creates a temporary
object URL from the returned bytes. There is no custom URI scheme or resource
protocol.

## Generated bindings

Rust command signatures and data types are authoritative:

```powershell
cargo run -p koharu --bin generate
```

Focused validation:

```powershell
cargo check -p koharu
bun x tsc --noEmit -p packages/koharu/tsconfig.json
cd packages/koharu
bun run test
```
