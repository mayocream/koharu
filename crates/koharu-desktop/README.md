# koharu-desktop

Desktop window, in-process browser, and GPU composition ownership for Koharu.

The crate links the official crates.io CEF 151 bindings behind an isolated
adapter. The application calls `dispatch_cef_process()` before logging, async
runtimes, or winit so Chromium helper modes can exit early. The browser process
itself is owned directly by the winit runtime and advances through CEF's
external message pump. Accelerated OSR imports D3D11 shared textures, DMA-BUF,
or IOSurface through cef-rs, then copies the result into a WGPU-owned texture
before CEF reclaims it. Software paint remains the fallback. Both paths enter
one bounded latest-frame mailbox, so backpressure stays correct without a
browser-host process, shared-memory pool, or IPC frame protocol.

The final application has one desktop window and one WGPU surface. CEF never
creates or presents an operating-system window.

`cef-dll-sys` downloads the pinned 151.3.16 distribution and builds
`libcef_dll_wrapper` with CMake and Ninja. Windows and Linux packages place the
CEF runtime files and `locales/` beside the executable. A macOS package places
`Chromium Embedded Framework.framework` and the standard `Koharu Helper`,
`Koharu Helper (GPU)`, `(Renderer)`, `(Plugin)`, and `(Alerts)` app bundles in
`Koharu.app/Contents/Frameworks`. CEF derives the base Helper path from
`main_bundle_path`; the role-specific bundles remain required for Chromium's
role-specific bundle identifiers, metadata, signing, and macOS process policy.
