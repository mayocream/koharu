# koharu-desktop

`koharu-desktop` is the native host for Koharu's Rust-owned editor viewport. It
combines a Winit window, one WGPU context, a transparent Wry child webview, and
`koharu-canvas`. React renders the application UI while Rust renders the page
and editor chrome beneath the transparent canvas rectangle.

The crate is the production version of the successful Wry viewport experiment.
The experimental `pocs/` workspace is intentionally gone; the runnable smoke
example in this crate is now the composition test.

## Ownership

```text
trusted React UI in transparent Wry child
    tools, gesture policy, panels, keyboard and text input
        | small JSON messages        ^ application events
        v                            |
application implementing Application
    Session, scene Commands, pipeline and UI policy
        | committed scene            ^ hit tests / mask snapshots
        v                            |
koharu-desktop ---------------- koharu-canvas
    Winit loop, Wry bridge,       scene drawing, overlays,
    shared WGPU device,           coordinate geometry, masks
    surface presentation
        |
        v
operating-system compositor
```

`koharu-desktop` owns:

- the native window, child webview, WGPU surface, adapter, device, and queue;
- the final canvas-texture-to-swapchain pass;
- webview sizing, device-pixel conversion, redraw coalescing, and surface
  recovery;
- a minimal trusted JavaScript bridge;
- ordered background encoding of completed mask strokes.

It does not own the `koharu-scene::Session`, application commands, tools,
selection policy, pipeline, model lifetime, project tabs, or a domain-specific
web protocol. Those belong to the executable's `Application` implementation.
The desktop crate never mirrors the scene in JavaScript and never transfers a
composited page image through IPC.

## Composition

The WGPU surface belongs to the Winit parent window. Wry covers that window as
a transparent native child. Opaque DOM panels hide the parent; the DOM element
reserved for the editor remains transparent, revealing the Rust-rendered
surface below. The operating system composes both native surfaces. Browser
pixels are not captured, copied, uploaded, or sampled by WGPU.

The web UI reports the editor element's logical `getBoundingClientRect()` and
`devicePixelRatio`. `koharu-desktop` validates them, converts them to physical
pixels once, and gives `koharu-canvas` a target of exactly that size. The final
pass places the canvas texture using a WGPU viewport and scissor rectangle. On
Windows the parent disables `WS_CLIPCHILDREN`, which is required for the WGPU
surface to remain visible beneath transparent regions of the WebView2 child.

The webview and parent are always transparent; this is a composition invariant,
not a configurable appearance option. The canvas texture contains
display-referred sRGB in `Rgba8Unorm`. The presenter converts it to linear while
writing an sRGB swapchain so canvas and export colors do not receive a second
gamma transform.

## Rust API

An executable supplies trusted HTML or a trusted URL and implements
`Application`. Its callbacks run on the native event-loop thread.

```rust,no_run
use anyhow::Result;
use koharu_desktop::{Application, DesktopContext, Frontend, Options};
use koharu_scene::{PageId, Session};

struct App {
    session: Session,
    page: PageId,
}

impl Application for App {
    fn started(&mut self, desktop: &mut DesktopContext<'_>) -> Result<()> {
        desktop.show_page(&self.session, self.page)?;
        Ok(())
    }

    fn message(
        &mut self,
        desktop: &mut DesktopContext<'_>,
        message: serde_json::Value,
    ) -> Result<()> {
        // Deserialize the application's own command enum and update Session.
        // Then call desktop.sync(&self.session, &changes).
        let _ = (desktop, message);
        Ok(())
    }
}

# fn main() -> Result<()> {
# let app = todo!();
koharu_desktop::run(
    Options {
        frontend: Frontend::Url("http://localhost:3000".into()),
        ..Options::default()
    },
    app,
)?;
# Ok(())
# }
```

Common operations are directly available on `DesktopContext`: `show_page`,
`sync`, `clear_page`, `set_view`, `set_overlays`, `submit_mask`, and `emit`.
`canvas()` exposes less common `koharu-canvas` operations without duplicating
wrappers; borrowing it also schedules a frame so a mutation is not silently
left unpresented. `DesktopHandle` is cloneable and may be used by worker threads
to emit an event, request a redraw, or exit.

Long-running pipeline/model work must not run in an `Application` callback.
Run it on its existing worker/runtime and return through `DesktopHandle`. Scene
commits should stay short. Loading a large page may read its encoded blobs before
the canvas moves decoding and GPU upload to background workers.

## Web bridge

The initialization script installs only two functions:

```javascript
window.koharu.send({ type: "select", element });

const unlisten = window.koharu.listen(event => {
  console.log(event.type, event.payload);
});
```

The shell reserves two incoming message shapes:

```javascript
window.koharu.send({
  type: "ready",
  dpr: devicePixelRatio,
  width: innerWidth,
  height: innerHeight,
});

const rect = canvasHost.getBoundingClientRect();
window.koharu.send({
  type: "viewport",
  x: rect.x,
  y: rect.y,
  width: rect.width,
  height: rect.height,
  dpr: devicePixelRatio,
});
```

All other JSON values go unchanged to `Application::message`; the application
should deserialize them into its own tagged enum. Rust-to-web events have the
shape `{ type, payload }` and are delivered through `window.koharu.listen`.
Malformed, non-finite, oversized (over 1 MiB), and invalid viewport messages are
logged and ignored. Application callback errors are fatal and make `run` return
the error.

This bridge is for interaction and state notifications, not image transport.
Scene images remain content-addressed bytes in `koharu-scene`; the canvas reads
and uploads them on the Rust side. `Frontend::Url` must point to trusted local
application content (a development server or packaged `file:` URL), because
that page receives the native bridge.

## Scheduling and performance

- The desktop creates exactly one adapter, device, and queue. The same
  `Arc<Device>` and `Arc<Queue>` are given to `koharu-canvas`.
- A frame is requested only after scene, view, overlay, resource, mask, resize,
  or bounded-transition work. Multiple requests before the next event-loop wake
  collapse into one Winit redraw.
- The event loop uses `ControlFlow::Wait`; an idle editor consumes no render
  loop CPU.
- The canvas target is only the physical editor rectangle, while the cheap
  final pass renders it into the full window surface.
- Presentation has no GPU readback, CPU page composition, browser-frame copy,
  HTTP image transfer, or second device.
- Resize and a zero-sized/minimized window suspend presentation without
  discarding canvas caches.
- A surface timeout schedules one retry; occlusion waits for Winit's visible
  event instead of spinning. Lost/outdated surfaces are reconfigured. A device
  loss is fatal in the initial implementation and the executable should restart
  the desktop host.

Completed mask strokes are immutable snapshots. `submit_mask` encodes them on
Rayon, serializes work for the same `(PageId, MaskPlane)`, and permits different
pages or planes to encode in parallel. `Application::mask_encoded` receives the
single-channel PNG or an error on the event-loop thread. The application then
stores it with scene commands, acknowledges its generation in the canvas, and
syncs the resulting `ChangeSet`. Encoding never blocks presentation, and FIFO
ordering prevents an older result from overtaking a newer stroke on the same
plane.

## Lifecycle

1. `run` creates the Winit event loop.
2. On resume, the parent window and WGPU surface are created before the Wry
   child so both share the proven native stacking order.
3. `Application::started` receives a fully initialized desktop.
4. The web page sends `ready` and reports its viewport. Every viewport report
   updates the canvas size and calls `Application::viewport_changed`.
5. Winit redraws only dirty frames. Wry remains responsible for DOM input,
   focus, accessibility, IME, and text fields.
6. `close_requested` may accept or cancel a native close. `DesktopHandle::exit`
   exits unconditionally.

The first implementation targets Windows and macOS child-webview composition.
Wry's child API is X11-only on Linux and requires GTK-loop integration; Wayland
needs a different embedding path before Linux can be claimed as supported.

## Testing

Pure tests cover message routing, malformed input, DPR rounding, viewport
clamping, and zero/invalid geometry. Compile and run the native smoke route with:

```text
cargo run -p koharu-desktop --example smoke
```

It opens opaque DOM chrome around a transparent center and renders a generated
page through `koharu-scene` and `koharu-canvas`. Resize the window and verify the
page remains aligned with the transparent rectangle. For an automated startup
check on a machine with a desktop and GPU, pass `--auto-exit`; the example exits
shortly after the webview reports ready:

```text
cargo run -p koharu-desktop --example smoke -- --auto-exit
```

Release validation should additionally cover Windows WebView2 and macOS WKWebView
focus/IME, non-integer DPR, monitor changes, minimization, surface loss, rapid
resizes, pointer-sample ordering, long mask queues, and packaged asset loading.
