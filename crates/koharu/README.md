# koharu

`koharu` is Koharu's native composition package. It owns process startup,
diagnostics, Tauri build integration, and application configuration.

## Application boundary

```text
React
  | direct Tauri commands
  | typed IPC channels
  v
koharu-app
  | Tauri-managed project, canvas, pipeline, jobs, and channel state
  +-> koharu-scene
  +-> koharu-desktop -> koharu-canvas
  +-> koharu-renderer -> raster / koharu-psd

koharu -> koharu-app + koharu-desktop
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
cargo run -p koharu-app --bin generate
```

Focused validation:

```powershell
cargo check -p koharu -p koharu-app -p koharu-desktop
bun x tsc --noEmit -p packages/koharu/tsconfig.json
cd packages/koharu
bun run test
```
