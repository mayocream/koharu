# koharu-canvas

`koharu-canvas` is Koharu's browser-only WebGPU/WASM editor viewport. It binds
the shared rasterizer to an `HTMLCanvasElement`, displays native-prepared frame
manifests, and owns only presentation resources, camera state, and transient
previews.

React owns tools, hit testing, selection, pointer gestures, and DOM controls.
The native application owns the scene, commits, undo, persistence, document
preparation, and durable validation. The browser never discovers fonts, decodes
document assets, or commits project state.

## Browser contract

`createCanvas(element)` initializes WebGPU asynchronously. A frame is installed
by staging a versioned manifest, fetching only its reported missing content
IDs, installing the independently encoded resources, and activating the stage
token. Resources are validated once and retained in bounded CPU and GPU caches
across page switches. Large resource decode and upload work is split across the
asynchronous per-resource calls. Raster packets are independently cached GPU-safe
tiles, so cold pages never require one page-sized WASM copy or WebGPU texture.

Activation is atomic: staging and resource delivery leave the current page
visible, a superseded token cannot activate, and an invalid or incomplete stage
does not replace the active frame. A replacement frame clears the pending
preview for the same page and a newer revision. `clear()` removes active and
staged presentation state without discarding reusable resources. Resize and
clear cancel color samples.

## Rendering and scheduling

The canvas uses `requestAnimationFrame` only while content is dirty or a GPU
readback needs polling. `render()` also provides a deterministic manual
presentation path for browser tests. Device loss is reported to JavaScript so
the frontend can dispose, recreate, and reinstall the last authoritative
frame.

The crate intentionally exports no native implementation. Native workspace
checks compile an empty shell; functional validation uses the
`wasm32-unknown-unknown` target and a WebGPU-capable browser.
