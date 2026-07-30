# Native desktop smoke tests

These Playwright tests attach to Koharu's running WebView2 instance. They are
kept as an opt-in composition check and are not part of the headless CI suite:
they require Windows, WebView2, a visible Winit window, and (for canvas cases)
a working WGPU adapter.

Start Koharu first, then run the suite from another terminal:

```powershell
bun run dev
bun run test:desktop
```

`bun run test:integration` remains as a compatibility alias. Prefer Rust unit
tests and Vitest for application behavior; add a case here only when it must
cross a real WebView, operating-system window, native dialog, or GPU surface.
