# @koharu/bridge

This package owns Koharu's browser/native boundary:

- `src/protocol.ts` is generated from the Rust Tauri command surface;
- `src/canvas.ts` adapts that protocol and `koharu-canvas` to the browser;
- `src/wasm` is ignored derived output produced by `wasm-pack`.

The bridge package builds only its WASM output. The root frontend tasks sequence that build before starting or building the consuming Next.js app. During development, `nodemon` rebuilds the bridge when `koharu-canvas` or `koharu-rasterizer` changes. `npm prefix` supplies the absolute workspace root directly to the package commands, so they do not depend on parent traversal or shell environment variables. Because the generated glue is imported from this package, Next.js includes it in the Turbopack module graph and refreshes the client after a successful rebuild.

Regenerate the protocol with `cargo run -p koharu-app --bin generate`. Do not edit generated protocol or WASM output by hand.
