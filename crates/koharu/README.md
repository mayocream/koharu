# koharu

`koharu` is Koharu's thin desktop process entrypoint. It dispatches CEF helper
processes before initializing diagnostics, then composes the transport-neutral
application with the desktop shell.

## Application boundary

```text
React static export
  | generated request/response + one sequenced event stream
  v
koharu-protocol
  v
koharu -> koharu-app -> scene / pipeline / renderer / agent
       -> koharu-desktop -> windowless CEF + canvas -> sole WGPU surface
```

`koharu-app` owns durable application state and use cases without browser,
window, or surface types. `koharu-desktop` owns exactly one winit window and
one presenter; its windowless CEF host supplies GPU-imported UI frames with a
software fallback, while `koharu-canvas` supplies the canvas texture. Binary
results use transferable attachments instead of base64.

## Generated bindings

Rust command signatures and data types are authoritative:

```powershell
cargo run -p koharu-protocol --bin generate
```

Focused validation:

```powershell
cargo check -p koharu
bun x tsc --noEmit --incremental false -p packages/koharu/tsconfig.json
```
