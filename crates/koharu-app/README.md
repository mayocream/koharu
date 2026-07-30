# koharu-app

`koharu-app` is Koharu's application layer. It turns user intent into semantic
scene revisions, projects those revisions into a small UI model, and coordinates
the desktop, canvas, renderer, and in-process pipeline.

The redesign is intentionally a clean break from the old scene-shaped desktop
protocol. UI code does not serialize scene components and application code does
not maintain a second document model.

## Ownership

```text
web UI
  |  intent commands / projection events
  v
koharu-app
  |-- Project -------- SceneSession, visible page, grouped history
  |-- projection ----- SceneSnapshot -> UI capability views
  |-- jobs ----------- pipeline, import, export, stop, progress
  |-- resources ------ authorized content-addressed thumbnails
  `-- app ------------ native lifecycle and desktop/canvas coordination
        |
        +-- koharu-scene       durable semantic state
        +-- koharu-pipeline    in-process model scheduling
        +-- koharu-renderer    deterministic scene rendering
        `-- koharu-desktop     window, bridge, and native canvas host
```

The scene is the only durable project state. The app owns ephemeral concerns
that do not belong in a document: which page is visible, camera state, running
jobs, pending mask encodes, settings UI, and undo grouping.

`koharu-app` does not own model implementations, rendering rules, GPU resource
policy, SQLite details, or React gesture algorithms.

## Project aggregate

`Project` is the headless application aggregate for one open `.khr` file. It
owns one `SceneSession` and exposes intent-oriented operations such as add text,
set translation, change typography, move entities, and delete pages.

Every durable edit follows the same path:

1. Read one immutable `SceneSnapshot`.
2. Validate the command's base revision.
3. Build one `ScenePatch` from the complete user intent.
4. Commit once.
5. Record the resulting revision as one undo group.
6. Derive a `ProjectDelta` from the scene change set.

Multi-entity changes are atomic. Pipeline and import jobs may create several
revisions; their revisions are recorded as one application-level undo group
when the job finishes. Redo is implemented by undoing the inverse revision, so
history uses the storage engine's durable operations instead of copied scene
state.

The visible page is reconciled after every refresh. If it disappears, the first
remaining page becomes visible; an empty project has no visible page.

## Desktop protocol

`protocol.rs` is an application API, not a persistence schema.

Commands express intent:

- `SetSourceText` corrects OCR text while preserving its detected language and
  promotes the text to user authorship.
- `SetTranslation` requires an explicit language tag.
- `SetTypography` carries semantic typography preferences, not glyph layout or
  renderer effects.
- `SetGeometry` carries entity geometry without exposing component slots.
- page and entity operations use the same stable scene `EntityId`.

Events expose projections:

- `PageSummary` is the navigator-sized page model.
- `PageView` is a complete projection of the visible page.
- `EntityView` is capability-based. Geometry, source text, translation,
  typography, region meaning, and image content are independent optional
  capabilities; there is no closed `Element` enum.
- `ProjectDelta` carries ordered page summaries and a complete visible-page
  replacement only when that page was affected.

This boundary lets new scene components be added without changing every app or
UI type. A capability is added to the projection only when the application has
a user-facing reason to expose it.

The TypeScript contract is generated from Rust:

```powershell
cargo run -p koharu-app --bin generate
```

`ui/lib/koharu/protocol.ts` is generated output and must not be edited by hand.

## Consistency and synchronization

Durable commands include the UI's base `Revision`. A stale command is rejected
with the current revision; the client discards queued edits and requests a full
synchronization. Interactions such as camera movement and live transform
previews are ephemeral and do not advance the scene revision. React sends
every move, resize, and rotation sample as a monotonically numbered, complete
set of absolute frames; Rust rejects malformed sets and ignores stale frames.

The main `SceneSession` is refreshed before each command and whenever a
background writer reports progress. Scene change sets drive three independent
updates:

- canvas synchronization;
- the UI projection delta;
- resource authorization, only when entity or asset references changed.

Affected page summaries are deduplicated. Entity-to-page ownership is cached
and refreshed only for structurally changed pages, so a text edit does not scan
the entire project.

## Jobs and concurrency

The app uses in-process jobs; there is no worker process.

- The `Pipeline` is constructed once and shared. Configuration changes replace
  it with a newly validated immutable pipeline.
- Pipeline work is asynchronous and uses page-stage dependencies, model locks,
  concurrent model admission, the resource monitor, and a stop token.
- Each completed pipeline page-stage patch commits immediately and advances the
  visible project. All revisions from the invocation form one undo group.
- Import runs on the blocking pool because file and image decoding are
  synchronous.
- Export has a dedicated serial actor that reuses one GPU renderer and its font
  and prepared-page caches.
- Export requests capture a `SceneSnapshot` at acceptance time, so an export is
  deterministic even if editing continues.
- Export may run alongside one project-writing job. Pipeline and import are
  mutually exclusive at the app boundary to avoid competing project commits.

Every long operation has a request ID, stop token, retained status, and progress
events. Closing or replacing a project stops all of its jobs. Late
events are ignored after their request leaves the active job table.

## Startup and settings

Application construction creates the long-lived pipeline and renderer resource
catalog, but model runtimes initialize after the desktop starts. The frontend
remains on its initialization view until `koharu_ml::init()` succeeds. Failures
retry automatically with bounded exponential backoff and jitter.

Pipeline and translation settings can change at runtime. A candidate
configuration is fully validated before files or credentials are updated. The
pipeline swaps to a new configuration generation, the canvas switches locale,
and the visible page projection is emitted again so translated text changes
immediately.

Stored credentials never cross into the webview. Settings projections expose
only whether each keychain entry is configured. A submitted credential edit is
explicitly `keep`, `set`, or `clear`, which prevents an untouched masked field
from accidentally deleting a stored key.

## Masks

Brush and text masks remain responsive in the canvas. A finished stroke is
encoded off the UI thread, then stored as a content-addressed scene asset in one
revision. Encodes for the same page and plane are ordered. Durable commands are
temporarily rejected while an encode is pending, preventing history or page
changes from interleaving with an uncommitted mask generation.

## Resource protocol

The web UI receives blob IDs, never filesystem paths. `koharu-resource` accepts
only content-addressed blobs referenced by the active snapshot and produces
bounded WebP thumbnails on a CPU pool. Requests are checked against both the
active project ID and an authorization set. The LRU cache is bounded to 64 MiB
and keyed by project, blob, and requested width.

Malformed asset components fail project presentation instead of being silently
omitted from authorization.

## Module map

- `project.rs` — scene commands, revision checks, history, and deltas.
- `projection.rs` — read-only scene-to-UI adapters.
- `protocol.rs` — generated Rust/TypeScript boundary types.
- `app.rs` — native lifecycle, command routing, and canvas coordination.
- `jobs.rs` and `jobs/*` — job supervision and import/pipeline/export adapters.
- `resources.rs` — trusted custom protocol and thumbnail cache.

## Invariants

- No durable UI mutation bypasses `SceneSession`.
- No scene component payload crosses the desktop protocol directly.
- A translation is always addressed by an explicit `LanguageTag`.
- Direct scene-edit commands produce at most one commit. Pipeline and import
  jobs intentionally commit incrementally and group their revisions for undo.
- Background snapshots are immutable; a background writer must commit through
  its own session and notify the main session after every commit.
- Renderer and pipeline output remain semantic scene patches, not app-specific
  side tables.
- UI resource access is allowlisted from the current snapshot.
- Native callbacks never trust a stale revision or an unknown job ID.

## Validation

```powershell
cargo test -p koharu-app --all-targets
cargo clippy -p koharu-app --all-targets --no-deps -- -D warnings
bun x tsc --noEmit -p ui/tsconfig.json
bun run --cwd ui test
bun run --cwd ui build
```
