# koharu

`koharu` is the native application and composition root. It wires the headless
`koharu-app` state machine to dialogs, background jobs, resources,
`koharu-desktop`, and `koharu-canvas`. Reusable protocol, project/session,
command-batching, history, projection, and error policy belongs in
`koharu-app`; operating-system and GPU adapters remain here.

This is a greenfield design. The current Tauri shell, generated RPC client,
OpenAPI schema, and old frontend scene API are not compatibility constraints.
The finished package uses `koharu-desktop` directly and has no local HTTP server
or Tauri command layer.

## Responsibilities

```text
React UI
    tools, gesture policy, selection, dialogs, text input, panels
       | small typed intents              ^ state/job events
       v                                  |
koharu::App  --------------------------------------------------+
    active Session, app protocol, history groups, job policy   |
       |             |                 |                        |
       v             v                 v                        |
koharu-scene   koharu-desktop    background runtime            |
                    |             pipeline / import / export    |
                    v                 |             |            |
              koharu-canvas <---------+             |            |
                    |                               |            |
                    v                               v            |
              koharu-renderer                 koharu-psd         |
                    ^                               |            |
                    +-------------------------------+------------+

Supporting services: koharu-config, koharu-secrets,
koharu-runtime, koharu-fonts, and optional koharu-ai.
```

The `koharu` entry package owns:

- startup, shutdown, logging, crash reporting, platform setup, and versioning;

`koharu-app` owns:

- the active project path and `koharu_scene::Session`;
- mapping UI intents into `koharu_scene::Commands`;
- applying short interactive commits and synchronizing their `ChangeSet` with
  `koharu-canvas`;
- application-level undo/redo grouping;
- pipeline, import, export, font-download, AI, and maintenance job policy;
- the small Rust/React protocol and its read-only UI projection;
- native file dialogs, recent projects, resource URLs, and update policy.

It does not reimplement anything owned by another crate. In particular there is
no second scene model, renderer facade, model registry, blob store, generic job
DAG, repository layer, service container, event bus, or application database.

## Crate wiring

| Crate | How `koharu` uses it |
| --- | --- |
| `koharu-app` | Owns the headless desktop protocol and active-project state, including scene command batching, revision history, projections, and stable errors. |
| `koharu-desktop` | Runs Winit/Wry, owns the canvas and shared WGPU context, and delivers native/UI events. |
| `koharu-scene` | Provides the only project state, SQLite file, commands, revisions, history, and blobs. |
| `koharu-canvas` | Handles the visible page, camera geometry, hit testing, overlays, and immediate mask editing. |
| `koharu-renderer` | Renders canvas text transitively and produces headless raster exports and thumbnails. |
| `koharu-pipeline` | Owns model selection, scheduling, progress, cancellation, and model lifetime. |
| `koharu-config` | Supplies live typed configuration handles. |
| `koharu-secrets` | Implements the platform credential-store backend used by translation credentials. |
| `koharu-runtime` | Supplies current HTTP clients, package paths, model/font downloads, and device discovery. |
| `koharu-fonts` | Browses and downloads fonts, then causes canvas/export font caches to be invalidated. |
| `koharu-psd` | Writes PSD exports from an immutable project revision. |
| `koharu-ai` | Runs optional image/assistant jobs and returns proposals; it never mutates a session directly. |

`koharu-ml`, `koharu-translator`, `koharu-torch`, `koharu-llama`,
`koharu-diffusion`, their `-sys` crates, and `koharu-bindgen` remain behind the
crates that implement those concerns. The executable must not depend on their
low-level APIs merely to forward calls.

## State ownership

There is one authority for each kind of state:

| State | Owner |
| --- | --- |
| Project pages, elements, images, masks, text, and history | active `Session` |
| Visible page pixels, camera, hover, transform geometry/state, previews, and mask strokes | `koharu-canvas` |
| Tools, pointer capture, gesture policy, selection, text-field drafts, panels, and dialogs | React |
| Pipeline models and loaded weights | `koharu-pipeline` on the background runtime |
| Runtime settings | typed `koharu-config::Config<T>` handles |
| Credentials | platform credential store |
| Job progress and cancellation | `koharu-app` plus the job that performs the work |

React keeps a read-only projection of scene metadata because inspectors,
navigators, and text fields need it. That projection is not another scene
authority: React cannot commit revisions, validate geometry, manage blobs,
resolve history, or decide whether an edit is legal. Images and masks are never
embedded in the JSON projection.

The initial implementation has one open project and one visible page. Adding
tabs later means adding multiple sessions to this composition root; it does not
justify a generic project manager today.

## Application shape

The reusable state is isolated from the native adapters:

```rust,ignore
struct App {
    project: Option<koharu_app::Project>,
    background: Background,
    jobs: HashMap<JobId, CancellationToken>,
}
```

`App` implements `koharu_desktop::Application` and adapts native callbacks to
`koharu_app::Project`. `Project` owns the `Session`, path, visible page, and
undo/redo groups without depending on Winit, Wry, WGPU, or native dialogs.
`Background` remains one purpose-built channel/runtime owner, not a generic
executor abstraction.

The executable is not a reusable Rust library. `main.rs` installs diagnostics,
loads configuration handles, creates `Background`, chooses the packaged or
development frontend, and calls `koharu_desktop::run`. Modules are private
unless an integration test genuinely needs a public entry point.

## React protocol

The web bridge carries application intents and events, not Rust function names
or serialized SQLite records. Two small Serde enums are enough:

```rust,ignore
#[serde(tag = "type", rename_all = "snake_case")]
enum UiMessage {
    Command {
        id: u64,
        base: Revision,
        command: UiCommand,
    },
    Interaction { interaction: CanvasInteraction },
}

#[serde(tag = "type", rename_all = "snake_case")]
enum UiEvent {
    Accepted { id: u64, revision: Revision },
    Rejected { id: u64, error: UiError },
    ProjectOpened {
        revision: Revision,
        project: ProjectHeader,
        pages: Vec<PageSummary>,
    },
    PageLoaded { revision: Revision, page: Page },
    ProjectChanged(ProjectDelta),
    ProjectClosed,
    JobChanged(JobStatus),
    SettingsChanged(SettingsView),
}
```

Persistent commands include their UI request ID and the scene revision on which
the UI based the edit. Representative `UiCommand` variants are `CreateProject`,
`OpenProject`, `ImportPages`, `SetTranslation`, `SetTextStyle`, `MoveElements`,
`DeleteElements`, `Undo`, `Redo`, `RunPipeline`, `CancelJob`, `Export`,
`SetPipelineConfig`, and `SetSecret`. They express user intent; the frontend
never sends raw `koharu_scene::Commands`, attachment maps, Revision payloads,
SQL, or model calls.

`CanvasInteraction` contains non-durable presentation operations such as view
changes, selection/hover overlays, hit tests, transform begin/update/cancel,
pointer samples, and mask-stroke begin/extend/finish. These do not require a
scene revision or create history. `FinishTransform` is a revision-aware command:
it takes the canvas-owned result and commits it through the normal application
path.

### UI projection

Opening a project sends a small project header and page summaries. Loading the
visible page sends that page and its elements. The UI does not eagerly receive
every text block in a large book. Later commits send a compact `ProjectDelta`
derived from `ChangeSet`:

- the current revision, project name, and page order;
- upserted or deleted summaries for touched pages;
- visible-page metadata and element order when that page changed;
- upserted or deleted values for touched elements on the visible page.

A page update contains its source/assets and element ID order, not another copy
of every element. An element update carries the actual `Element`. Changes to an
unloaded page update its navigator summary; selecting it later loads its current
complete value. These small application DTOs are justified because eagerly
sending an entire book, or sending a complete page after every text edit, would
be needlessly expensive. Blob bytes, decoded pixels, renderer data, and pipeline
state are excluded.

Events are emitted in revision order. If React observes a revision gap or
reloads, it requests the current headers and visible page instead of attempting
to repair a partial projection. A rejected command removes its optimistic
preview and uses the latest delta supplied by Rust.

The protocol stays hand-written while it is this small. Rust Serde round-trip
tests and TypeScript fixture tests must cover every tag. Do not restore OpenAPI,
an HTTP client generator, or a schema framework merely to move these two enums
across an in-process webview.

## Interactive edit flow

Every durable interactive edit follows one path:

1. React finishes or debounces a gesture and sends a typed intent with its base
   revision. During a transform, React only forwards physical pointer positions;
   the canvas owns the preview geometry.
2. `App` calls `Session::refresh` first. Any background commits are synchronized
   to the canvas and emitted to React.
3. `App` checks the request revision and referenced page/element values.
4. It builds one `Commands` batch, usually through `session.edit()`.
5. `Session::apply` commits atomically to SQLite.
6. `desktop.sync(&session, &changes)` incrementally invalidates the canvas.
7. `App` emits `ProjectChanged`, followed by `Accepted`.

Transform dragging uses canvas-owned overlays during pointer movement. On
pointer-up, `FinishTransform` asks the canvas for its result and the application
commits that frame batch once. Mask painting stays immediate in the tiled canvas
and commits one single-channel blob after `Application::mask_encoded`. Text
fields may keep a React draft, but should debounce or commit on blur rather than
append one SQLite/history revision for every key repeat.

When a mask encode completes, `App` calls `Commands::set_asset`, retains the
returned `BlobId`, applies the commands, acknowledges the canvas generation,
then synchronizes the `ChangeSet`. An old generation never replaces a newer
stroke.

There is no autosave timer: a successful scene command is already a durable
SQLite transaction. UI drafts and unfinished gestures are intentionally not
project state.

## Background work and concurrent writes

The Winit thread owns the active `Session` and performs only short metadata and
interactive commits. It must never await model inference, network requests,
large imports, raster/PSD export, thumbnail generation, backup, GC, or model
unloading.

One dedicated Tokio runtime owns a reusable `Pipeline` and receives a small
closed `Job` enum. CPU image work may use Rayon inside the owning crate. A job
that needs project data opens its own `Session` for the same `.khr` path and
captures the revision at job start. It never takes a mutex around the UI's
session and never holds a SQLite transaction during expensive work.

Pipeline progress is emitted after a committed wave. When `App` receives a
native `ProjectAdvanced` event, it refreshes its session, synchronizes the
canvas, emits the scene delta, and only then reports progress to React. Import
and AI jobs follow the same rule when they commit scene commands. Export jobs
open a stable read snapshot and report which revision they exported; they do
not mutate the scene.

SQLite and `koharu-scene` arbitrate simultaneous writers. If an interactive edit
or background wave loses a revision race, it fails explicitly. `App` refreshes
and reports the conflict; it does not silently rebase model output or use
last-writer-wins. Already committed pipeline waves remain visible and are
reported by `RunError::committed_revisions`.

Only one pipeline run is active initially because `Pipeline` already owns its
run lock and accelerator policy. Downloads may proceed concurrently. GPU-heavy
exports should queue behind inference until profiling shows the devices can run
both without harming canvas responsiveness.

### Required native event hook

Background results must return directly to `App`; they must not be serialized to
React and bounced back into Rust. `koharu-desktop` therefore needs one typed
application-event hook, conceptually:

```rust,ignore
trait Application {
    type Event: Send + 'static;
    fn event(&mut self, desktop: &mut DesktopContext<'_, Self::Event>, event: Self::Event)
        -> anyhow::Result<()>;
}

desktop_handle.send_event(BackgroundEvent::ProjectAdvanced { job })?;
```

The host remains unaware of job semantics. It merely carries the event through
its Winit proxy. The same host boundary should expose native file drops and an
asynchronous custom-protocol responder. Adding these narrow hooks is preferable
to polling, global shared state, or a JavaScript round trip.

## Undo and redo

`koharu-scene` provides reversible durable revisions; `koharu` decides which
revisions form one user action.

- one ordinary edit creates one undo group;
- all successfully committed waves from one pipeline run form one group;
- importing several selected pages is one group;
- undo reverts a group newest-first and records the resulting revision as the
  redo action;
- any new edit clears the redo stack.

The initial undo/redo stacks are process-local. Reopening a file starts a new UI
history session while the SQLite commit log remains available for safe storage,
GC, and future history tooling. Do not add a cursor or duplicate inverse-command
format in this crate.

## Rendering, thumbnails, and export

The main editor never receives rasterized text or page images in React.
`koharu-canvas` reads the active session and renders beneath the transparent DOM
workspace. React's old DOM image, text-sprite, and mask-canvas composition paths
must be removed; the center workspace becomes an interaction surface only.

Small DOM images are still useful for navigator thumbnails and image pickers.
They use a read-only Wry custom resource protocol rather than JSON or base64:

```text
koharu-resource://project/<project-id>/blob/<blob-id>?width=160
```

The handler accepts only the active project and referenced IDs, caps output
dimensions and byte size, and performs decode/render work off the Winit thread.
Immutable thumbnails are cached by `(BlobId, size)`. A page navigator uses the
page's source or clean `BlobId`; it does not headlessly composite every page in
the background. Add revision-keyed composite thumbnails only if a measured UI
need justifies their render cost. The protocol cannot expose arbitrary
filesystem paths. Full-resolution data remains in SQLite and is read directly
by Rust for canvas, pipeline, and export work.

Raster export asks `koharu-renderer` to render an immutable project revision.
Source/clean export reads the corresponding scene blob. PSD export converts
that same revision into `koharu_psd::PsdDocument` and calls `koharu-psd`. Export
does not require or update `PageAsset::Rendered`; rendering is not a pipeline
stage and an export is not an editor mutation.

Optional `koharu-ai` output is treated as a proposal containing bytes and
metadata. Only an explicit application/user decision converts it into an image
element or page asset through scene commands.

## Configuration and secrets

Load independently owned sections once and keep their handles:

```rust,ignore
let app = koharu_config::load::<AppConfig>("app")?;
let pipeline = koharu_config::load::<PipelineConfig>("pipeline")?;
let translation = TranslationConfig::load()?;
let http = koharu_config::load::<HttpConfig>("http")?;
```

The background runtime receives the same live pipeline handle. A run snapshots
the latest configuration through `koharu-pipeline`; changing the translator or
model affects the next run and never mutates a running plan. Callers do not keep
cloned config values or hold read/write guards across `.await`. A settings edit
uses `write()`, validates the complete typed value, calls `save()`, and emits
the current settings view.

`koharu_translator::TranslationConfig` loads and saves its `SecretString`
provider keys through the platform credential store. The trusted settings UI
receives their values
so users can reveal and edit them; the TOML file stores only a redacted value.
Logs, errors, Sentry events, recent-file metadata, job descriptions, and
resource URLs must not contain secrets or page text/image content.

UI-only preferences such as theme, panel sizes, and shortcuts may stay in
browser local storage. Model, network, secret, recent-project, and other
Rust-runtime settings belong in the Rust configuration system. Do not mirror
the same setting in both stores.

After a font download completes, `App` invalidates canvas fonts and the export
font service, requests one redraw, and emits updated availability. The font file
itself is not inserted into the scene database.

## Startup and shutdown

Startup order is deterministic:

1. initialize Sentry, the panic hook, tracing, and platform fixes;
2. load live app, HTTP, and pipeline configuration handles;
3. create the background runtime, `Pipeline`, font service, and job channel;
4. select the trusted frontend: a localhost development URL only in debug, or
   bundled `ui/out` through a read-only custom protocol in release;
5. construct `App` and call `koharu_desktop::run`;
6. after the webview reports ready, emit version/platform/settings/job state and
   optionally reopen the last project.

The release webview never navigates to arbitrary remote content because loaded
content receives the native bridge. Native open/save dialogs return paths to
Rust; large files are not copied through JavaScript.

Closing a project cancels its jobs and waits for mask commits before dropping
the session. Closing the application first refuses the native close, requests
background cancellation/model unload, and exits through `DesktopHandle` after
the worker acknowledges shutdown. A bounded timeout may force exit after logs
and crash state are flushed. Scene commits need no separate save prompt.

The redesign removes Tauri dependencies, plugins, configuration, capabilities,
and `build.rs`. Native dialogs use `rfd`, URLs/files use the narrow platform
helpers already selected by the application, and updating becomes an explicit
application job instead of a Tauri plugin side effect.

## Performance rules

- No model inference, download, image decode, export, GC, or backup on the Winit
  thread.
- No `Arc<Mutex<Session>>`; independent SQLite sessions plus revisions handle
  concurrency.
- No full-project JSON after an ordinary edit and no image bytes in JSON.
- No persistent commit for pointer movement, hover, selection, or transform
  preview.
- One scene batch and one canvas sync per completed user gesture.
- Coalesce repeated background progress/resource events before waking Winit.
- Reuse the pipeline and loaded processors; configuration changes reconcile them
  before the next run.
- Cache only derived thumbnails/resources with bounded byte limits and immutable
  blob keys.
- Keep the desktop event loop idle when the canvas and jobs are idle.

These rules keep the UI responsive without introducing a second mutable scene
or a large orchestration framework.

## Failure rules

- Bad UI input is rejected with a stable application error code; detailed error
  chains stay in Rust logs.
- A stale UI revision receives the latest delta and must retry deliberately.
- Missing optional fonts/assets degrade only the affected preview or export.
- A corrupt or incompatible project never replaces the currently open session.
- A failed import/pipeline wave commits nothing for that wave; earlier reported
  revisions remain valid.
- Export uses the captured revision even if editing continues.
- Device loss is fatal according to the initial `koharu-desktop` contract;
  surface loss is recovered by the host.
- Secrets and project content are redacted from telemetry.

## Current source layout

`crates/koharu-app/src/protocol.rs` owns the shared Rust/TypeScript protocol and
`crates/koharu-app/src/project.rs` owns the headless project controller. Native
application dispatch, jobs, resources, dialogs, and UI assets also live in
`koharu-app`. The `koharu` package contains only entrypoint concerns.

```text
crates/koharu-app/src/
  app.rs
  project.rs
  protocol.rs
  jobs.rs
  jobs/
  resources.rs

crates/koharu/src/
  main.rs
  panic.rs
  sentry.rs
  tracing.rs
  version.rs
  windows.rs
```

<details>
<summary>Historical design sketch</summary>

```text
src/
â”œâ”€â”€ main.rs          startup and `koharu_desktop::run`
â”œâ”€â”€ app.rs           `App` and Application callback dispatch
â”œâ”€â”€ background.rs    closed Job/BackgroundEvent enums and runtime owner
â”œâ”€â”€ export.rs        scene-to-raster/PSD wiring
â”œâ”€â”€ resources.rs     trusted UI assets and bounded thumbnail protocol
â”œâ”€â”€ panic.rs
â”œâ”€â”€ sentry.rs
â”œâ”€â”€ tracing.rs
â”œâ”€â”€ version.rs
â””â”€â”€ windows.rs
```
</details>

Keep a module only when it owns real policy. Do not create `services/`,
`repositories/`, `managers/`, generic request handlers, one-line wrappers, or a
module per UI command.

## Testing

Pull requests use deterministic headless tests. A passing test must not depend
on a WebView or silently skip because no GPU adapter was found.

- `cargo test -p koharu-app` round-trips the protocol and uses
  `Session::memory` to verify commands, deltas, undo/redo groups, and stable
  errors without constructing the native application.
- `cargo test -p koharu-desktop --lib` verifies IPC decoding, frontend routing,
  viewport conversion, and native-operation selection without creating a
  window or WebView.
- `cargo test -p koharu-canvas --lib` verifies move, rotated resize, rotation,
  geometry, hit testing, masks, and other CPU state. The real renderer test is
  marked ignored and probes pixels after move, resize, and rotation previews;
  run it
  explicitly with `cargo test -p koharu-canvas -- --ignored` on a machine with
  a GPU. It fails if its requested adapter cannot be created.
- `bun run test:ui` uses a fake native client to verify React state,
  interactions, and protocol messages without WebView2.
- Background tests use temporary `.khr` files and fake processors to verify
  refresh ordering, partial pipeline commits, cancellation, and writer
  conflicts.
- Export tests compare raster and PSD structure from the same scene revision.
- Resource tests reject unknown projects/blobs, traversal attempts, and
  excessive dimensions.
- Application integration tests verify that a commit emits canvas sync before
  its UI delta and that job progress follows any newly committed scene state.
- `tests/integration-tests` is retained for optional native composition smoke
  tests. Run it locally with `bun run test:desktop`; it is not a headless CI
  gate. `bun run test:integration` remains an alias for compatibility.

### Automating the Windows desktop with Playwright

Windows development builds expose the Wry WebView2 through the Chrome DevTools
Protocol (CDP), and Playwright attaches to that endpoint with
`chromium.connectOverCDP`. Tests use Playwright locators, assertions, keyboard,
and mouse APIs rather than issuing raw CDP commands. The workspace Cargo
configuration passes `--remote-debugging-port=0`, so WebView2 chooses an unused
loopback port for each browser process. This setting applies to processes
launched by Cargo; it does not make a packaged Koharu executable remotely
debuggable. The root development dependencies include `@playwright/test`; no
Playwright browser download is needed because the test attaches to the WebView2
Runtime that Koharu already uses.

Start the UI and native host normally:

```powershell
bun run dev
```

Wait for the Koharu window to appear. With the `release-with-debug` profile,
WebView2 writes its selected port and browser WebSocket path here:

```text
target/release-with-debug/koharu.exe.WebView2/EBWebView/DevToolsActivePort
```

The first line is the port and the second line is the browser WebSocket path.
Other Cargo profiles use the corresponding directory below `target/`. Discover
the page target from PowerShell with:

```powershell
$portFile = 'target/release-with-debug/koharu.exe.WebView2/EBWebView/DevToolsActivePort'
$port = Get-Content -LiteralPath $portFile -TotalCount 1
Invoke-RestMethod "http://127.0.0.1:$port/json/list" |
  Select-Object title, url, webSocketDebuggerUrl
```

The target should be titled `Koharu`, point at `http://localhost:3000/`, and
contain a `webSocketDebuggerUrl`. If the file is missing, confirm that the
native process was started after the Cargo configuration changed and that the
WebView2 child process is running. Do not run two copies of the same executable
against the same WebView2 user-data directory.

The reusable Playwright suite lives under `tests/integration-tests`. With
`bun run dev` still running, execute it from another terminal:

```powershell
bun run test:desktop
```

The fixture reads `DevToolsActivePort`, attaches one Playwright client per
worker with `noDefaults`, locates the Koharu page, and waits for the native
bridge. The configuration uses one worker because WebView2 exposes one browser
endpoint for this desktop process. `browser.close()` only disconnects that
client; the externally owned Koharu process stays open.

The desktop cases verify the bridge and persistent controls, create a project
through the native Windows Save As dialog, open and dismiss Settings, maximize
and restore the real Winit window, and check the welcome actions when no project
is open. Generated projects are written below the Playwright output directory,
and every reversible window, project, or dialog change is cleaned up after the
test. Run one case by title with:

```powershell
bun run test:desktop -- --grep "maximizes and restores"
```

#### Canvas gestures

`canvas.spec.ts` finds the editor through its accessible `Koharu canvas` label,
performs a middle-button pan and wheel zoom, waits for the resulting
`view_changed` events from Rust, and restores Fit Window in `finally`. The case
skips when no project is open. Open a project manually, or start the UI server
and pass a temporary test project to the native executable in separate
terminals before running the suite:

```powershell
bun run dev:ui
cargo run -p koharu --bin koharu --profile release-with-debug -- C:\path\to\fixture.khr
bun run test:desktop
```

#### Automation boundaries

- Playwright controls content inside WebView2. It can click the custom titlebar
  buttons because those buttons send native window actions through Koharu's
  bridge.
- Playwright mouse movement does not move the physical Windows cursor. Testing
  the operating-system titlebar drag loop requires Windows UI automation or
  real system input; use Playwright for canvas drags instead.
- Native open/save dialogs are outside WebView2 and require Windows UI
  automation. Prefer starting Koharu with a temporary project path in tests.
- The Rust canvas is a separate WGPU surface beneath the transparent WebView2
  child. Playwright screenshots cover the DOM surface, not the final
  OS-composited WGPU canvas. Verify canvas pixels through renderer tests or
  native window capture.
- Use a unique WebView2 user-data folder when a test runner launches processes
  concurrently. Set `WEBVIEW2_USER_DATA_FOLDER` on each child process and let
  the runner read that instance's `DevToolsActivePort` file.
- Keep remote debugging limited to development and tests. Do not add the flag
  to packaged application startup.

The first milestone is complete when the Tauri shell and old UI scene transport
are gone; a `.khr` file can be created/opened, edited, undone, run through the
pipeline, rendered in the Rust-owned canvas, and exported; configuration and
secrets can change at runtime; and React contains interaction policy but no
scene storage or compositing logic.
