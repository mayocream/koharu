# koharu-app

`koharu-app` owns Koharu's application-level behavior.

`Project` owns the active scene session, path, visible page, command batching,
undo/redo groups, deltas, and stable errors, while `protocol` owns the shared
Rust/TypeScript desktop contract. Tests remain headless by behavior: they do
not create Winit, Wry, WebView, or WGPU objects.

```powershell
cargo test -p koharu-app
```

The crate also owns the application adapter, background jobs, trusted resource
protocol, dialogs, embedded UI assets, and wiring to
`koharu-desktop`/`koharu-canvas`. The `koharu` executable remains responsible
only for CLI parsing, diagnostics, platform startup, and calling
`koharu_app::app::run`.

The TypeScript protocol is generated from this crate:

```powershell
cargo run -p koharu-app --bin generate
```
